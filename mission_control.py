#!/usr/bin/env python3
"""
CESAROPS Mission Control
========================
The "knob turner" interface for the wreck-hunting pipeline.

An agent (or human operator) submits a mission spec — either as a JSON file
or CLI args — and this module handles the full pipeline:

  1. Weather fetch + storm/calm/post-storm classification
  2. Smart date selection (calm-only, post-storm-only, or both)
  3. Satellite data download (via universal_downloader.py)
  4. Scan routing (lake_erie_scan, lake_michigan_scan, …)
  5. DB write with condition + scan_group tags
  6. Summary report

Agent JSON schema (what an LLM agent should emit)
--------------------------------------------------
{
  "mission_id": "MB2_PLUME_APR2026",
  "target_name": "Marquette and Bessemer No. 2",
  "bbox": [41.80, -82.50, 42.50, -80.00],    // [lat_min, lon_min, lat_max, lon_max]
  "date_range": ["2024-04-01", "2024-04-30"],
  "sensors": ["hls"],                          // hls | sentinel1 | swot | icesat2 | all
  "weather_filter": "calm_and_post_storm",     // calm | post_storm | storm | all | calm_and_post_storm
  "passes": [1, 2, 3, 5, 6, 7],               // which scan passes to run (null = all)
  "knobs": {
    "hc_threshold":              1.8,   // z-score threshold for hydrocarbon detection
    "silt_erasure_threshold":    2.5,   // B11/B12 z-score for sub-silt metallic signature
    "displacement_min_delta":    2.0,   // min Δz for water surface displacement (Pass 7)
    "mussel_clearspot_top_n":   30,    // top-N mussel clearspot detections per scene
    "max_download_results":    200,    // cap on CMR granule search results
    "post_storm_days":           3,    // how many post-storm days to include
    "calm_max_wind_kmh":        15,    // wind threshold for "calm" classification
    "storm_min_wind_kmh":       28     // wind threshold for "storm" classification
  },
  "output": {
    "db_path": null,   // null = auto (outputs/<mission_id>/scans.db)
    "scan_group": null // null = auto-assigned per detection type
  }
}

CLI usage
---------
  # Full mission from JSON spec:
  python mission_control.py --spec missions/mb2_apr2026.json

  # Quick mission from flags (agent-friendly):
  python mission_control.py \
    --mission-id MB2_TEST \
    --bbox 41.80,-82.50,42.50,-80.00 \
    --dates 2024-04-01 2024-04-30 \
    --sensors hls \
    --weather calm_and_post_storm \
    --download-only

  # Dry run (show what would be downloaded, no actual fetch):
  python mission_control.py --spec missions/mb2_apr2026.json --dry-run
"""

import argparse
import json
import os
import subprocess
import sys
from datetime import date, datetime, timedelta
from pathlib import Path
from typing import Optional

REPO = Path(__file__).resolve().parent

# ─────────────────────────────────────────────────────────────────────────────
# Default knob values  (ML can tune these from external config)
# ─────────────────────────────────────────────────────────────────────────────
DEFAULT_KNOBS = {
    "hc_threshold":             1.8,
    "silt_erasure_threshold":   2.5,
    "displacement_min_delta":   2.0,
    "mussel_clearspot_top_n":  30,
    "max_download_results":   200,
    "post_storm_days":          3,
    "calm_max_wind_kmh":       15.0,
    "storm_min_wind_kmh":      28.0,
}

# Supported sensor → universal_downloader --sensors arg mapping
SENSOR_MAP = {
    "hls":       "hls",
    "sentinel1": "sentinel1",
    "sentinel2": "sentinel2",
    "swot":      "swot",
    "icesat2":   "icesat2",
    "all":       "hls sentinel1 swot icesat2",
}


# ─────────────────────────────────────────────────────────────────────────────
# Mission spec loading
# ─────────────────────────────────────────────────────────────────────────────

def load_spec(spec_path: Path) -> dict:
    with open(spec_path, encoding="utf-8") as f:
        return json.load(f)


def spec_from_args(args) -> dict:
    """Build a minimal spec dict from CLI args."""
    bbox = [float(x) for x in args.bbox.split(",")]
    knobs = dict(DEFAULT_KNOBS)
    if args.knobs:
        knobs.update(json.loads(args.knobs))
    return {
        "mission_id":     args.mission_id or f"MISSION_{date.today().strftime('%Y%m%d')}",
        "target_name":    args.target or "",
        "bbox":           bbox,
        "date_range":     [args.dates[0], args.dates[1]],
        "sensors":        args.sensors or ["hls"],
        "weather_filter": args.weather or "calm_and_post_storm",
        "passes":         None,
        "knobs":          knobs,
        "output":         {"db_path": None, "scan_group": None},
    }


# ─────────────────────────────────────────────────────────────────────────────
# Step 1 — Weather + date selection
# ─────────────────────────────────────────────────────────────────────────────

def select_dates(spec: dict, verbose: bool = True) -> dict:
    """
    Fetch weather, classify days, return grouped date lists.

    Returns:
        {
          "calm":          [...dates...],
          "post_storm_1":  [...],
          "post_storm_2":  [...],
          "post_storm_3":  [...],
          "storm":         [...],
          "all_conditions": {date: condition, ...},
        }
    """
    bbox      = spec["bbox"]
    dr        = spec["date_range"]
    knobs     = {**DEFAULT_KNOBS, **spec.get("knobs", {})}
    lat_c     = (bbox[0] + bbox[2]) / 2
    lon_c     = (bbox[1] + bbox[3]) / 2

    try:
        from weather_service import (
            get_historical_weather,
            tag_storm_calm_pairs,
            classify_day_condition,
        )
    except ImportError as e:
        print(f"[WEATHER] weather_service not available: {e}")
        return {"all_conditions": {}}

    if verbose:
        print(f"\n[WEATHER] Fetching {dr[0]} → {dr[1]} at ({lat_c:.3f}, {lon_c:.3f})")

    wx = get_historical_weather(lat_c, lon_c, dr[0], dr[1])
    conditions = tag_storm_calm_pairs(
        wx,
        post_storm_days=int(knobs["post_storm_days"]),
    )

    grouped: dict = {
        "calm": [], "storm": [], "transitional": [],
        "post_storm_1": [], "post_storm_2": [], "post_storm_3": [],
        "all_conditions": conditions,
        "weather_raw": {w["date"]: w for w in wx},
    }
    for d, cond in conditions.items():
        if cond in grouped:
            grouped[cond].append(d)

    if verbose:
        summary = {k: len(v) for k, v in grouped.items()
                   if isinstance(v, list) and k != "all_conditions"}
        print(f"  Date classification: {summary}")

    return grouped


def filter_dates_by_weather(all_dates: list, grouped: dict, weather_filter: str) -> list:
    """
    Return subset of all_dates that match the requested weather_filter.

    weather_filter values:
      calm                — only calm days
      post_storm          — post_storm_1/2/3 combined
      storm               — only storm days
      calm_and_post_storm — calm + all post_storm buckets  (default)
      all / ""            — every date regardless of weather
    """
    if not weather_filter or weather_filter == "all":
        return sorted(all_dates)

    keep = set()
    wf = weather_filter.lower()

    if "calm" in wf:
        keep.update(grouped.get("calm", []))
    if "post_storm" in wf:
        keep.update(grouped.get("post_storm_1", []))
        keep.update(grouped.get("post_storm_2", []))
        keep.update(grouped.get("post_storm_3", []))
    if wf == "storm":
        keep.update(grouped.get("storm", []))

    # Intersect with dates that actually have data
    return sorted(d for d in all_dates if d in keep) if keep else sorted(all_dates)


# ─────────────────────────────────────────────────────────────────────────────
# Step 2 — Download
# ─────────────────────────────────────────────────────────────────────────────

def run_download(spec: dict, date_list: list, dry_run: bool = False) -> Path:
    """
    Invoke universal_downloader.py for the selected dates.
    Returns the output directory Path.
    """
    bbox      = spec["bbox"]
    knobs     = {**DEFAULT_KNOBS, **spec.get("knobs", {})}
    mission   = spec["mission_id"].lower()
    sensors   = spec.get("sensors", ["hls"])
    sensor_str = " ".join(SENSOR_MAP.get(s, s) for s in sensors)

    # Output directory scoped to mission
    dl_dir = REPO / "downloads" / "missions" / mission
    dl_dir.mkdir(parents=True, exist_ok=True)

    dr = spec["date_range"]
    bbox_str = f"{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}"

    cmd = [
        sys.executable,
        str(REPO / "universal_downloader.py"),
        "--bbox",    bbox_str,
        "--dates",   dr[0], dr[1],
        "--sensors", *sensor_str.split(),
        "--max-results", str(int(knobs["max_download_results"])),
        "--output",  str(dl_dir),
    ]

    print(f"\n[DOWNLOAD] {'(DRY RUN) ' if dry_run else ''}Sensors: {sensor_str}")
    print(f"           Bbox: {bbox_str}")
    print(f"           Dates: {dr[0]} → {dr[1]}")
    print(f"           Date filter kept: {len(date_list)} days")
    print(f"           Output: {dl_dir}")

    if dry_run:
        print(f"  cmd: {' '.join(str(c) for c in cmd)}")
        return dl_dir

    result = subprocess.run(cmd, cwd=str(REPO))
    if result.returncode != 0:
        print(f"[DOWNLOAD] WARNING: downloader exited {result.returncode}")

    return dl_dir


# ─────────────────────────────────────────────────────────────────────────────
# Step 3 — Scan routing
# ─────────────────────────────────────────────────────────────────────────────

def run_scan(spec: dict, dl_dir: Path, date_grouped: dict, dry_run: bool = False):
    """
    Run the scan passes directly (in-process) over the downloaded TIFFs.

    This imports the scan engine and runs the full pass suite with all
    knobs, weather conditions, and DB writes already wired.
    """
    knobs     = {**DEFAULT_KNOBS, **spec.get("knobs", {})}
    bbox      = spec["bbox"]
    mission   = spec["mission_id"]
    passes    = spec.get("passes")  # None means all

    # Resolve output DB
    db_override = spec.get("output", {}).get("db_path")
    out_dir = REPO / "outputs" / mission.lower()
    out_dir.mkdir(parents=True, exist_ok=True)
    db_path = Path(db_override) if db_override else out_dir / "scans.db"

    print(f"\n[SCAN] Mission: {mission}")
    print(f"  TIFF source:  {dl_dir}")
    print(f"  DB output:    {db_path}")
    print(f"  Passes:       {passes or 'all'}")
    print(f"  Knobs:        {json.dumps(knobs, indent=2)}")

    if dry_run:
        print("[SCAN] Dry run — no scan executed.")
        return

    # ── Import scan engine components ─────────────────────────────────────────
    try:
        from lake_michigan_scan import (
            process_hydrocarbon_bands,
            process_tiff_with_coords,
            compute_nauticuvs_pass,
            compute_stumpf_pass,
            _flag_known_wreck,
            _is_linear_wake,
        )
        from lake_erie_scan import (
            detect_swir_silt_erasure,
            detect_mussel_clearspot,
            _flag_mb2_zone,
            extract_date_from_path,
            write_day_to_db,
            write_displacement_to_db,
            _erie_db_connect,
        )
    except ImportError as e:
        print(f"[SCAN] FATAL: Could not import scan engine: {e}")
        return

    from collections import defaultdict
    from rasterio.warp import transform as warp_transform
    import rasterio

    # ── DB path override ───────────────────────────────────────────────────────
    # lake_erie_scan uses the module-level ERIE_DB constant, so we monkey-patch
    # it here to redirect output to this mission's DB.
    import lake_erie_scan as _les
    _les.ERIE_DB = db_path

    # ── Discover TIFFs ─────────────────────────────────────────────────────────
    tiffs = sorted(dl_dir.rglob("*.tif"))
    print(f"\n[SCAN] Found {len(tiffs)} TIFFs under {dl_dir}")

    by_date: dict = defaultdict(list)
    for t in tiffs:
        d = extract_date_from_path(t)
        if d:
            by_date[str(d)].append(t)
        else:
            by_date["unknown"].append(t)

    all_conditions = date_grouped.get("all_conditions", {})
    wx_raw         = date_grouped.get("weather_raw",    {})

    all_detections = []
    _mb2_pixel_map: dict = {}               # (lat, lon) rounded to 3dp → {bucket: {zscore, date}}
    DISP_THRESHOLD = float(knobs["displacement_min_delta"])

    _SKIP_UPPER = {'FMASK', '.B11.', '.SWIR16.', '.SWIR22.',
                   '.SCL.', '.QA_PIXEL.', '.NIR08.', '.NIR.'}

    scan_dates = sorted(by_date.keys())
    print(f"[SCAN] Scanning {len(scan_dates)} date groups...")

    for date_str in scan_dates:
        tiffs_today = by_date[date_str]
        condition   = all_conditions.get(date_str, 'unknown')
        wx_day      = wx_raw.get(date_str)
        print(f"\n  {date_str}  cond={condition}  ({len(tiffs_today)} TIFFs)")

        day_detections = []

        # ── PASS 1: Standard anomaly ─────────────────────────────────────────
        if passes is None or 1 in passes:
            std_tiffs = [t for t in tiffs_today if
                         not any(tag in t.name.upper() for tag in _SKIP_UPPER)]
            for tiff in std_tiffs:
                tname = tiff.name.upper()
                is_thermal = 'B10' in tname or 'THERMAL' in tname
                is_blue    = '.B02.' in tname or '.BLUE.' in tname
                thresh     = 2.0 if is_thermal else (1.2 if is_blue else 1.5)
                try:
                    dets = process_tiff_with_coords(
                        tiff, threshold=thresh, scan_bbox=bbox, top_n=200,
                        cold_sink_mode=is_thermal)
                    for d in dets:
                        d["scan_date"] = date_str
                        d["mb2_zone"] = _flag_mb2_zone(d["lat"], d["lon"])
                    day_detections.extend(dets)
                except Exception as e:
                    print(f"    [P1] {e}")

        # ── PASS 2: Hydrocarbon B11+B04 ──────────────────────────────────────
        if passes is None or 2 in passes:
            b11_tiffs = [t for t in tiffs_today if
                         ('.B11.' in t.name.upper() or '.SWIR16.' in t.name.upper()) and
                         'FMASK' not in t.name.upper()]
            for b11 in b11_tiffs:
                pname = b11.name.lower()
                b04 = Path(str(b11).replace('.swir16.tif', '.red.tif')
                           .replace('.B11.tif', '.B04.tif'))
                try:
                    dets = process_hydrocarbon_bands(b11, b04)
                    for d in dets:
                        d["scan_date"] = date_str
                        d["mb2_zone"] = _flag_mb2_zone(d["lat"], d["lon"])
                    day_detections.extend(dets)
                except Exception as e:
                    print(f"    [P2] {e}")

        # ── PASS 3: Stumpf bathymetric ───────────────────────────────────────
        if passes is None or 3 in passes:
            blue_tiffs = [t for t in tiffs_today if
                          ('.B02.' in t.name.upper() or '.BLUE.' in t.name.upper()) and
                          'FMASK' not in t.name.upper()]
            for blue in blue_tiffs:
                green = Path(str(blue).replace('.B02.tif', '.B03.tif')
                             .replace('.blue.tif', '.green.tif'))
                try:
                    dets = compute_stumpf_pass(blue, green, scan_bbox=bbox)
                    for d in dets:
                        d["scan_date"] = date_str
                        d["mb2_zone"] = _flag_mb2_zone(d["lat"], d["lon"])
                    day_detections.extend(dets)
                except Exception as e:
                    print(f"    [P3] {e}")

        # ── PASS 5: SWIR silt erasure (MB2 zone) ─────────────────────────────
        if passes is None or 5 in passes:
            b12_bands = [t for t in tiffs_today if
                         ('.B12.' in t.name.upper() or '.SWIR22.' in t.name.upper()) and
                         'FMASK' not in t.name.upper()]
            for b12 in b12_bands:
                b11 = Path(str(b12).replace('.B12.', '.B11.').replace('.swir22.', '.swir16.'))
                try:
                    dets = detect_swir_silt_erasure(
                        b11, b12, scan_bbox=bbox,
                        top_n=int(knobs["mussel_clearspot_top_n"]))
                    for d in dets:
                        d["scan_date"] = date_str
                    day_detections.extend(dets)
                except Exception as e:
                    print(f"    [P5] {e}")

        # ── PASS 6: Mussel clearspot (calm days preferred) ───────────────────
        if passes is None or 6 in passes:
            blue_tiffs_mb2 = [t for t in tiffs_today if
                              ('.B02.' in t.name.upper() or '.BLUE.' in t.name.upper()) and
                              'FMASK' not in t.name.upper()]
            for blue in blue_tiffs_mb2:
                try:
                    dets = detect_mussel_clearspot(
                        blue, scan_bbox=bbox,
                        top_n=int(knobs["mussel_clearspot_top_n"]))
                    for d in dets:
                        d["scan_date"] = date_str
                    day_detections.extend(dets)
                except Exception as e:
                    print(f"    [P6] {e}")

        # ── Stamp condition + write DB ────────────────────────────────────────
        for d in day_detections:
            d['condition'] = condition
        try:
            write_day_to_db(day_detections, date_str, condition, wx_day)
        except Exception as e:
            print(f"    [DB] {e}")

        # ── Accumulate Pass 7 pixel map ───────────────────────────────────────
        if condition in ('calm', 'post_storm_1', 'post_storm_2', 'post_storm_3'):
            bucket = 'calm' if condition == 'calm' else 'post_storm'
            for d in day_detections:
                if not d.get('mb2_zone'):
                    continue
                key = (round(d['lat'], 3), round(d['lon'], 3))
                entry = _mb2_pixel_map.setdefault(key, {})
                prev_z = entry.get(bucket, {}).get('zscore', -999)
                if abs(d.get('zscore', 0)) > abs(prev_z):
                    entry[bucket] = {
                        'zscore': d.get('zscore', 0),
                        'date':   date_str,
                        'type':   d.get('type'),
                    }

        all_detections.extend(day_detections)

    # ── PASS 7: Displacement cross-comparison ─────────────────────────────────
    if passes is None or 7 in passes:
        displacement_hits = []
        for (lat_k, lon_k), buckets in _mb2_pixel_map.items():
            calm_e = buckets.get('calm')
            ps_e   = buckets.get('post_storm')
            if not calm_e or not ps_e:
                continue
            delta = ps_e['zscore'] - calm_e['zscore']
            if delta >= DISP_THRESHOLD:
                displacement_hits.append({
                    'lat':               lat_k,
                    'lon':               lon_k,
                    'type':              'water_displacement',
                    'calm_date':         calm_e['date'],
                    'post_storm_date':   ps_e['date'],
                    'calm_zscore':       calm_e['zscore'],
                    'post_storm_zscore': ps_e['zscore'],
                    'displacement_delta': round(delta, 4),
                    'mb2_zone':          _flag_mb2_zone(lat_k, lon_k),
                    'confidence':        min(1.0, delta / 5.0),
                    'scan_date':         f"{calm_e['date']}_to_{ps_e['date']}",
                    'condition':         'post_storm',
                    'zscore':            ps_e['zscore'],
                })

        print(f"\n[PASS 7] Displacement hits: {len(displacement_hits)} "
              f"(Δz≥{DISP_THRESHOLD}, {len(_mb2_pixel_map)} grid cells)")
        try:
            write_displacement_to_db(displacement_hits)
            write_day_to_db(displacement_hits, 'multi_date', 'post_storm')
        except Exception as e:
            print(f"  [PASS 7 DB] {e}")
        all_detections.extend(displacement_hits)

    # ── Summary ───────────────────────────────────────────────────────────────
    hc_total   = sum(1 for d in all_detections if d.get('type') == 'hydrocarbon')
    mb2_total  = sum(1 for d in all_detections if d.get('mb2_zone'))
    disp_total = sum(1 for d in all_detections if d.get('type') == 'water_displacement')
    print(f"\n{'='*70}")
    print(f"SCAN COMPLETE — {mission}")
    print(f"  Total detections:    {len(all_detections)}")
    print(f"  Hydrocarbon:         {hc_total}")
    print(f"  M&B2 zone:           {mb2_total}")
    print(f"  Displacement (P7):   {disp_total}")
    print(f"  DB:                  {db_path}")
    print(f"{'='*70}")
    return all_detections


# ─────────────────────────────────────────────────────────────────────────────
# Entrypoint
# ─────────────────────────────────────────────────────────────────────────────

def run_mission(spec: dict, dry_run: bool = False, download_only: bool = False,
                scan_only: bool = False, verbose: bool = True):
    """
    Execute a full mission end-to-end.

    Parameters
    ----------
    spec         : mission spec dict (from JSON or CLI)
    dry_run      : print plan without executing downloads or scans
    download_only: fetch data then stop (no scan)
    scan_only    : skip download, assume data already present
    verbose      : print progress
    """
    print(f"\n{'='*70}")
    print(f"MISSION CONTROL — {spec['mission_id']}")
    print(f"  Target:  {spec.get('target_name', 'unspecified')}")
    print(f"  Bbox:    {spec['bbox']}")
    print(f"  Dates:   {spec['date_range']}")
    print(f"  Sensors: {spec.get('sensors', ['hls'])}")
    print(f"  Weather: {spec.get('weather_filter', 'calm_and_post_storm')}")
    print(f"{'='*70}")

    # Step 1: weather + date classification
    date_grouped = select_dates(spec, verbose=verbose)

    # Step 2: filter to dates matching weather_filter
    all_dates = list(date_grouped.get("all_conditions", {}).keys())
    selected  = filter_dates_by_weather(
        all_dates, date_grouped,
        spec.get("weather_filter", "calm_and_post_storm")
    )
    print(f"\n[DATE FILTER] {len(selected)}/{len(all_dates)} days selected "
          f"(filter: {spec.get('weather_filter', 'calm_and_post_storm')})")
    if selected:
        print(f"  First: {selected[0]}   Last: {selected[-1]}")

    # Step 3: download
    dl_dir = REPO / "downloads" / "missions" / spec["mission_id"].lower()
    if not scan_only:
        dl_dir = run_download(spec, selected, dry_run=dry_run)
    else:
        print(f"\n[DOWNLOAD] Skipped (scan_only=True) — using {dl_dir}")

    if download_only and not dry_run:
        print("\n[MISSION] Download complete. Stopping (--download-only).")
        return

    # Step 4: scan
    run_scan(spec, dl_dir, date_grouped, dry_run=dry_run)


def main():
    parser = argparse.ArgumentParser(
        description="CESAROPS Mission Control — bbox+dates → download → scan → DB",
        formatter_class=argparse.RawDescriptionHelpFormatter,
        epilog=__doc__,
    )

    # Spec file or inline args
    group = parser.add_mutually_exclusive_group()
    group.add_argument("--spec", type=Path,
                       help="Path to mission JSON spec file")
    group.add_argument("--mission-id", metavar="ID",
                       help="Quick mission: mission ID string")

    # Quick-mission args (used when --spec not provided)
    parser.add_argument("--bbox",    metavar="LAT_MIN,LON_MIN,LAT_MAX,LON_MAX",
                        help="Bounding box (comma-separated floats)")
    parser.add_argument("--dates",   nargs=2, metavar=("START", "END"),
                        help="Date range: START_DATE END_DATE (YYYY-MM-DD)")
    parser.add_argument("--target",  help="Human-readable target name")
    parser.add_argument("--sensors", nargs="+", default=["hls"],
                        choices=list(SENSOR_MAP.keys()),
                        help="Sensors to download (default: hls)")
    parser.add_argument("--weather",
                        choices=["calm", "post_storm", "storm",
                                 "calm_and_post_storm", "all"],
                        default="calm_and_post_storm",
                        help="Weather filter (default: calm_and_post_storm)")
    parser.add_argument("--knobs",   metavar="JSON",
                        help="JSON string of knob overrides, e.g. '{\"hc_threshold\": 2.0}'")

    # Mode flags
    parser.add_argument("--download-only", action="store_true",
                        help="Fetch data but do not run scan")
    parser.add_argument("--scan-only",     action="store_true",
                        help="Skip download, scan existing data")
    parser.add_argument("--dry-run",       action="store_true",
                        help="Print plan without executing")
    parser.add_argument("--quiet",         action="store_true",
                        help="Suppress verbose progress output")

    args = parser.parse_args()

    # Load or build spec
    if args.spec:
        spec = load_spec(args.spec)
    elif args.mission_id and args.bbox and args.dates:
        spec = spec_from_args(args)
    else:
        parser.print_help()
        sys.exit(1)

    # Merge CLI knobs into spec
    if hasattr(args, "knobs") and args.knobs and not args.spec:
        overrides = json.loads(args.knobs)
        spec.setdefault("knobs", {}).update(overrides)

    run_mission(
        spec,
        dry_run=args.dry_run,
        download_only=args.download_only,
        scan_only=args.scan_only,
        verbose=not args.quiet,
    )


if __name__ == "__main__":
    main()
