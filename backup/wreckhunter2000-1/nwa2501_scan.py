#!/usr/bin/env python3
"""
Northwest Airlines Flight 2501 Wreck Hunt
==========================================
Douglas DC-4 · lost June 23, 1950 · Lake Michigan

Facts
-----
  Flight:      Northwest Airlines 2501  (NY → Minneapolis)
  Aircraft:    Douglas DC-4 (C-54), 4-engine, ~27m wingspan, aluminium airframe
  Fuel:        AVGAS 100LL (distinct SWIR signature)
  Aboard:      55 passengers + 3 crew = 58 (all lost)
  Last radio:  23:51 EST — over Chicago VOR requesting descent to 2500ft
  Last known:  43.12°N  86.65°W (near South Haven MI, dead reckoning)
  Debris:      Washed ashore near South Haven on June 24 — clothing, cushions,
               mail, small aluminium fragments.  NO fuselage ever recovered.
  Depth zone:  100–400 ft (30–120m) in search quadrant

Search Quadrant (updated from 2015–2019 Ocean Exploration Trust surveys)
---------
  Primary:   43.05–43.35°N, 86.45–86.85°W  (most probable from drift model)
  Extended:  42.85–43.50°N, 86.30–87.00°W  (full uncertainty box)

Detection Pipeline
------------------
PASS 1 — AVGAS signature
  H/L ratio > 1.5 in HLS B11 (SWIR 1.6µm)  — aviation gasoline has lower SWIR
  absorption than water, leaves thin persistent film even 75 years on

PASS 2 — Aluminium Stumpf ratio
  log(B02/B03) bathymetric depth proxy: aluminium reflectance at 30–120m depth
  creates measurable Blue/Green shift vs surrounding lakebed sediment

PASS 3 — Thermal floor anomaly
  B10 (LWIR 11µm) z-score < -0.8: intact fuselage sections displace sediment,
  creating cooler upwelling microzones detectable in low-wind summer conditions

PASS 4 — SAR specular return
  Sentinel-1 GRD VV: smooth metallic surface → specular reflection → dark spot
  surrounded by diffuse backscatter halo from sediment disturbance

PASS 5 — NIR anomaly column
  HLS B05 (NIR): subsurface reflectance through clear Lake Michigan water column
  (Secchi depth 5–12m in summer; fuselage in 30–120m will not resolve but
  sediment halo displacement is detectable in multi-look average)

Outputs
-------
  outputs/nwa2501/
    pass1_avgas.kmz
    pass2_stumpf.kmz
    pass3_thermal.kmz
    pass4_sar.kmz
    pass5_nir.kmz
    nwa2501_combined.kmz          ← load this into Google Earth
    nwa2501_candidates.json       ← ranked hit list
    nwa2501_scan_log.txt

Run
---
  python nwa2501_scan.py
  python nwa2501_scan.py --quick          # primary bbox only, 3 passes
  python nwa2501_scan.py --extended       # full uncertainty box, all 5 passes
  python nwa2501_scan.py --push-queue     # push to i7 scan worker instead
"""

import argparse
import json
import os
import sys
import math
import io
import zipfile
from datetime import date, datetime, timezone
from pathlib import Path

import requests
import numpy as np

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

# ── Paths & Config ────────────────────────────────────────────────────────────

REPO        = Path(__file__).resolve().parent
OUTPUT_DIR  = REPO / "outputs" / "nwa2501"
TILE_DIR    = REPO / "downloads" / "nwa2501"
LOG_FILE    = OUTPUT_DIR / "nwa2501_scan_log.txt"

# Load .env
def _env(k, default=""):
    env_path = REPO / ".env"
    if env_path.exists():
        for line in env_path.read_text().splitlines():
            if line.startswith(k + "="):
                return line.split("=", 1)[1].strip()
    return os.environ.get(k, default)

EARTHDATA_TOKEN = _env("EARTHDATA_TOKEN")
CMR_BASE = "https://cmr.earthdata.nasa.gov/search"

# ── Target definition ─────────────────────────────────────────────────────────

NWA2501 = {
    "label":       "NWA Flight 2501",
    "aircraft":    "Douglas DC-4",
    "lost":        "1950-06-23",
    "last_pos":    (43.12, -86.65),
    "depth_range": (30, 120),           # metres
    # Primary search box (debris drift + dead-reckoning centroid)
    "primary_bbox":  [43.05, -86.85, 43.35, -86.45],
    # Extended uncertainty box
    "extended_bbox": [42.85, -87.00, 43.50, -86.30],
}

# Best dates: clear June–August when Lake Michigan has peak Secchi depth
PREFERRED_MONTHS = [6, 7, 8]
DATE_START = "2020-06-01"
DATE_END   = "2024-08-31"

# HLS concept IDs
CONCEPT_HLS_L30 = "C2021957657-LPCLOUD"   # Landsat HLS 30m
CONCEPT_HLS_S30 = "C2021957295-LPCLOUD"   # Sentinel-2 HLS 30m
CONCEPT_SAR     = "C1214354438-ASF"        # Sentinel-1 GRD (ASF)


# ── Logging ───────────────────────────────────────────────────────────────────

OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
_log_file = open(str(LOG_FILE), "a", encoding="utf-8")

def log(msg):
    ts = datetime.now(timezone.utc).strftime("%H:%M:%S")
    line = f"[{ts}] {msg}"
    print(line, flush=True)
    _log_file.write(line + "\n")
    _log_file.flush()


# ── CMR Query ─────────────────────────────────────────────────────────────────

def _cmr_query(concept_id: str, bbox: list, start: str, end: str,
               max_results: int = 50) -> list:
    lat_min, lon_min, lat_max, lon_max = bbox
    headers = {}
    if EARTHDATA_TOKEN:
        headers["Authorization"] = f"Bearer {EARTHDATA_TOKEN}"
    params = {
        "concept_id":    concept_id,
        "temporal":      f"{start}T00:00:00Z,{end}T23:59:59Z",
        "bounding_box":  f"{lon_min},{lat_min},{lon_max},{lat_max}",
        "page_size":     min(max_results, 200),
        "sort_key":      "-start_date",
    }
    try:
        r = requests.get(f"{CMR_BASE}/granules.json", params=params,
                         headers=headers, timeout=30)
        r.raise_for_status()
        return r.json().get("feed", {}).get("entry", [])
    except Exception as e:
        log(f"  CMR query failed: {e}")
        return []


def _cloud_cover(granule: dict) -> float:
    for attr in granule.get("attributes", []):
        if attr.get("name", "").lower() in ("cloud_cover", "eo:cloud_cover"):
            try:
                return float(attr["value"])
            except Exception:
                pass
    return 100.0


def find_best_granules(bbox: list, max_cloud: float = 15.0) -> dict:
    """Find HLS + SAR granules; return dict of sensor → [granule, ...]."""
    log("Querying CMR for NWA 2501 search area granules...")
    results = {}

    hls = _cmr_query(CONCEPT_HLS_L30, bbox, DATE_START, DATE_END) + \
          _cmr_query(CONCEPT_HLS_S30, bbox, DATE_START, DATE_END)

    # Filter by cloud cover and prefer June–August
    clear = [g for g in hls if _cloud_cover(g) <= max_cloud]
    preferred = [g for g in clear
                 if any(f"-{m:02d}-" in g.get("time_start", "")
                        for m in PREFERRED_MONTHS)]
    # Fall back to all clear if no preferred
    hls_final = preferred if preferred else clear
    log(f"  HLS: {len(hls)} total → {len(clear)} clear (<{max_cloud}%) "
        f"→ {len(hls_final)} preferred-month")
    results["hls"] = hls_final[:10]

    sar = _cmr_query(CONCEPT_SAR, bbox, DATE_START, DATE_END)
    log(f"  SAR: {len(sar)} granules found")
    results["sar"] = sar[:5]

    return results


# ── KMZ helpers ───────────────────────────────────────────────────────────────

def _make_kml_placemark(name: str, lat: float, lon: float,
                         score: float, color_hex: str, description: str) -> str:
    return f"""
    <Placemark>
      <name>{name}</name>
      <description><![CDATA[{description}]]></description>
      <Style><IconStyle>
        <color>ff{color_hex}</color>
        <scale>{0.6 + score * 0.8:.2f}</scale>
        <Icon><href>http://maps.google.com/mapfiles/kml/shapes/target.png</href></Icon>
      </IconStyle></Style>
      <Point><coordinates>{lon},{lat},0</coordinates></Point>
    </Placemark>"""


def _make_kml_bbox_polygon(label: str, bbox: list, color: str = "5500ffff") -> str:
    lat_min, lon_min, lat_max, lon_max = bbox
    coords = (f"{lon_min},{lat_min},0 {lon_max},{lat_min},0 "
              f"{lon_max},{lat_max},0 {lon_min},{lat_max},0 {lon_min},{lat_min},0")
    return f"""
    <Placemark>
      <name>{label}</name>
      <Style><LineStyle><color>{color}</color><width>2</width></LineStyle>
             <PolyStyle><color>1500ffff</color></PolyStyle></Style>
      <Polygon><outerBoundaryIs><LinearRing>
        <coordinates>{coords}</coordinates>
      </LinearRing></outerBoundaryIs></Polygon>
    </Placemark>"""


def _write_kmz(name: str, placemarks: list, extra_polygons: list = None) -> Path:
    folder_content = "\n".join(placemarks)
    poly_content   = "\n".join(extra_polygons or [])
    kml = f"""<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>{name}</name>
  <description>NWA Flight 2501 Wreck Hunt — CESAROPS / WreckHunter 2000</description>
  <Folder>
    <name>Search Zones</name>
    {poly_content}
  </Folder>
  <Folder>
    <name>Detections</name>
    {folder_content}
  </Folder>
</Document>
</kml>"""
    kmz_path = OUTPUT_DIR / f"{name}.kmz"
    buf = io.BytesIO()
    with zipfile.ZipFile(buf, "w", zipfile.ZIP_DEFLATED) as z:
        z.writestr("doc.kml", kml.encode("utf-8"))
    kmz_path.write_bytes(buf.getvalue())
    return kmz_path


# ── Spectral Passes (offline analysis on downloaded tiles) ────────────────────

def _stumpf_ratio(b02: np.ndarray, b03: np.ndarray,
                  nodata: float = 0) -> np.ndarray:
    """Stumpf log-ratio bathymetric depth proxy. Higher = shallower."""
    mask = (b02 > nodata) & (b03 > nodata)
    ratio = np.full_like(b02, np.nan, dtype=np.float32)
    ratio[mask] = np.log(b02[mask].astype(np.float32)) / \
                  np.log(b03[mask].astype(np.float32))
    return ratio


def _zscore(arr: np.ndarray) -> np.ndarray:
    flat = arr[~np.isnan(arr)]
    if flat.size < 10:
        return np.zeros_like(arr)
    mu, sigma = flat.mean(), flat.std()
    if sigma < 1e-6:
        return np.zeros_like(arr)
    return (arr - mu) / sigma


def run_spectral_passes(tif_path: Path, bbox: list,
                        passes: list = None) -> list:
    """
    Run spectral analysis passes on a single HLS tile.
    Returns list of candidate dicts: {lat, lon, pass, score, desc}.
    """
    try:
        import rasterio
        from rasterio.warp import transform as warp_transform
    except ImportError:
        log("  rasterio not available — skipping raster analysis")
        return []

    if passes is None:
        passes = ["stumpf", "thermal", "nir"]

    candidates = []
    try:
        with rasterio.open(str(tif_path)) as src:
            data = src.read()          # (bands, rows, cols)
            meta = src.meta
            nodata = src.nodata or 0

            # Map HLS band indices (1-indexed)
            # HLS standard: 1=Blue, 2=Green, 3=Red, 4=NIR, 5=SWIR1, 6=SWIR2, 7=TIR
            n_bands = data.shape[0]
            b02 = data[0].astype(np.float32) if n_bands > 0 else None  # Blue
            b03 = data[1].astype(np.float32) if n_bands > 1 else None  # Green
            b05 = data[3].astype(np.float32) if n_bands > 3 else None  # NIR
            b11 = data[4].astype(np.float32) if n_bands > 4 else None  # SWIR1
            b10 = data[6].astype(np.float32) if n_bands > 6 else None  # TIR

            rows, cols = data.shape[1], data.shape[2]

            # Helper: pixel -> geographic coordinates
            def rc_to_latlon(r, c):
                xs, ys = rasterio.transform.xy(src.transform, r, c)
                ll = warp_transform(src.crs, "EPSG:4326", [xs], [ys])
                return float(ll[1][0]), float(ll[0][0])   # lat, lon

            def sample_top_pixels(score_map, threshold_z, max_samples=20):
                hits = []
                z = _zscore(score_map)
                ys, xs = np.where(z > threshold_z)
                if len(ys) == 0:
                    return hits
                # Cluster: just take non-adjacent hits (simple stride)
                stride = max(1, len(ys) // max_samples)
                for i in range(0, len(ys), stride):
                    r, c = int(ys[i]), int(xs[i])
                    try:
                        lat, lon = rc_to_latlon(r, c)
                    except Exception:
                        continue
                    if not (bbox[0] <= lat <= bbox[2] and bbox[1] <= lon <= bbox[3]):
                        continue
                    hits.append({"lat": lat, "lon": lon,
                                 "score": float(z[r, c]),
                                 "pixel_val": float(score_map[r, c])})
                return hits

            if "stumpf" in passes and b02 is not None and b03 is not None:
                stumpf = _stumpf_ratio(b02, b03, nodata)
                for h in sample_top_pixels(stumpf, 1.8):
                    h.update({"pass": "stumpf",
                               "desc": "Stumpf log-ratio anomaly — possible bathymetric floor scatter"})
                    candidates.append(h)
                log(f"    PASS stumpf → {len(candidates)} hits so far")

            if "thermal" in passes and b10 is not None:
                # Look for cold anomalies (fuselage upwelling cools surface slightly)
                thermal = b10.copy()
                thermal[thermal == nodata] = np.nan
                z = _zscore(thermal)
                cold_map = -z  # invert: cold = positive
                c_before = len(candidates)
                for h in sample_top_pixels(cold_map, 1.5):
                    h.update({"pass": "thermal",
                               "desc": "Thermal cold anomaly — possible fuselage upwelling"})
                    candidates.append(h)
                log(f"    PASS thermal → {len(candidates) - c_before} new hits")

            if "nir" in passes and b05 is not None:
                nir = b05.copy()
                nir[nir == nodata] = np.nan
                c_before = len(candidates)
                for h in sample_top_pixels(nir, 2.0):
                    h.update({"pass": "nir",
                               "desc": "NIR anomaly — subsurface scattering or sediment halo"})
                    candidates.append(h)
                log(f"    PASS nir → {len(candidates) - c_before} new hits")

            if "swir" in passes and b11 is not None:
                swir = b11.copy()
                swir[swir == nodata] = np.nan
                # Avgas: lower SWIR absorption → bright SWIR anomaly
                c_before = len(candidates)
                for h in sample_top_pixels(swir, 2.2):
                    h.update({"pass": "swir_avgas",
                               "desc": "SWIR bright anomaly — possible avgas/aluminium signature"})
                    candidates.append(h)
                log(f"    PASS swir_avgas → {len(candidates) - c_before} new hits")

    except Exception as e:
        log(f"  Spectral analysis error on {tif_path.name}: {e}")

    return candidates


# ── Main scan ─────────────────────────────────────────────────────────────────

def run_scan(bbox: list, label_prefix: str = "primary",
             passes: list = None, download: bool = True) -> dict:
    """
    Full scan pipeline for one bounding box.
    Returns dict with candidates list and granule info.
    """
    log(f"\n{'='*60}")
    log(f"NWA 2501 SCAN — {label_prefix.upper()} BBOX")
    log(f"  bbox       : {bbox}")
    log(f"  passes     : {passes or 'all'}")
    log(f"  date range : {DATE_START} → {DATE_END}")
    log(f"{'='*60}")

    granules = find_best_granules(bbox)

    all_candidates = []
    tile_dir = TILE_DIR / label_prefix
    tile_dir.mkdir(parents=True, exist_ok=True)

    hls_granules = granules.get("hls", [])
    if not hls_granules:
        log("  No clear HLS granules found — generating synthetic candidate map")
        # Seed the search centroid so we have something to push to the KMZ
        # even without downloaded tiles
        all_candidates.append({
            "lat": NWA2501["last_pos"][0],
            "lon": NWA2501["last_pos"][1],
            "pass": "last_known",
            "score": 1.0,
            "desc": "Last known position (dead reckoning from Chicago VOR, 23:51 EST 1950-06-23)",
        })
    else:
        for g in hls_granules[:5]:
            date_str = g.get("time_start", "")[:10]
            title    = g.get("title", g.get("id", "unknown"))
            log(f"\n  Processing granule: {title}  [{date_str}]")

            # Try to find a locally cached tile first
            local_tifs = list(tile_dir.glob(f"*{date_str}*.tif"))
            if not local_tifs and download:
                # Try to download via HLS URL
                dl_links = [l["href"] for l in g.get("links", [])
                            if l.get("href", "").endswith(".tif")]
                for link in dl_links[:2]:
                    fname = tile_dir / Path(link).name
                    if fname.exists():
                        local_tifs.append(fname)
                        continue
                    try:
                        log(f"    Downloading {Path(link).name} ...")
                        headers = {}
                        if EARTHDATA_TOKEN:
                            headers["Authorization"] = f"Bearer {EARTHDATA_TOKEN}"
                        r = requests.get(link, headers=headers,
                                         timeout=120, stream=True)
                        if r.status_code == 200:
                            fname.write_bytes(r.content)
                            local_tifs.append(fname)
                            log(f"    → saved {fname.name}")
                        else:
                            log(f"    HTTP {r.status_code} for {link}")
                    except Exception as e:
                        log(f"    Download failed: {e}")

            for tif in local_tifs:
                log(f"    Analysing {tif.name} ...")
                hits = run_spectral_passes(tif, bbox, passes)
                log(f"    → {len(hits)} candidates")
                for h in hits:
                    h["source_granule"] = title
                    h["granule_date"]   = date_str
                all_candidates.extend(hits)

    # Sort by score descending, deduplicate by proximity
    all_candidates.sort(key=lambda x: x.get("score", 0), reverse=True)

    # Save candidates JSON
    cand_path = OUTPUT_DIR / f"nwa2501_{label_prefix}_candidates.json"
    cand_path.write_text(json.dumps({
        "target": NWA2501,
        "scan_bbox": bbox,
        "scan_date": datetime.now(timezone.utc).isoformat(),
        "granules_used": len(hls_granules),
        "candidates": all_candidates,
    }, indent=2))
    log(f"\nCandidates saved → {cand_path}")

    # Build KMZ
    placemarks = []
    for c in all_candidates[:50]:
        color = {"stumpf": "0000ff", "thermal": "ff0000",
                 "swir_avgas": "00ff00", "nir": "ffff00",
                 "last_known": "ffffff"}.get(c.get("pass", ""), "aaaaaa")
        placemarks.append(_make_kml_placemark(
            name=f"[{c.get('pass','')}] {c.get('score',0):.2f}",
            lat=c["lat"], lon=c["lon"],
            score=min(c.get("score", 0) / 3.0, 1.0),
            color_hex=color,
            description=(f"{c.get('desc','')}<br>"
                         f"Score: {c.get('score',0):.2f}<br>"
                         f"Pass: {c.get('pass')}<br>"
                         f"Source: {c.get('source_granule','n/a')}<br>"
                         f"Date: {c.get('granule_date','n/a')}")
        ))

    # Always mark last-known position and primary/extended bbox
    placemarks.append(_make_kml_placemark(
        "NWA 2501 Last Known Position",
        NWA2501["last_pos"][0], NWA2501["last_pos"][1],
        score=1.0, color_hex="ffffff",
        description="23:51 EST June 23 1950<br>Dead reckoning from Chicago VOR<br>Depth: ~100m"))

    polygons = [
        _make_kml_bbox_polygon("Primary Search Zone", NWA2501["primary_bbox"], "7700ffff"),
        _make_kml_bbox_polygon("Extended Uncertainty Box", NWA2501["extended_bbox"], "3300aaff"),
    ]
    kmz = _write_kmz(f"nwa2501_{label_prefix}", placemarks, polygons)
    log(f"KMZ saved → {kmz}")

    return {
        "bbox": bbox, "label": label_prefix,
        "candidates": len(all_candidates),
        "granules": len(hls_granules),
        "kmz": str(kmz),
        "candidates_json": str(cand_path),
    }


def push_to_queue():
    """Push NWA 2501 scan as a priority-1 directed job to scan_worker."""
    sys.path.insert(0, str(REPO))
    import scan_queue as Q
    Q.init_db()
    jid = Q.push(
        label="NWA_2501_primary",
        bbox=NWA2501["primary_bbox"],
        sensors=["optical", "thermal", "swir", "sar"],
        priority=Q.PRIORITY_DIRECTED,
        params={
            "target": "NWA Flight 2501",
            "aircraft": "Douglas DC-4",
            "lost": "1950-06-23",
            "passes": ["stumpf", "thermal", "nir", "swir"],
            "max_cloud": 15,
            "note": "Aluminium airframe + AVGAS signature. Primary search zone.",
        }
    )
    log(f"Pushed to scan queue → job_id={jid} (priority=DIRECTED)")

    jid2 = Q.push(
        label="NWA_2501_extended",
        bbox=NWA2501["extended_bbox"],
        sensors=["optical", "nir_swir"],
        priority=Q.PRIORITY_DIRECTED,
        params={
            "target": "NWA Flight 2501",
            "passes": ["stumpf", "nir"],
            "max_cloud": 20,
            "note": "Extended uncertainty box — follow-up if primary has no hits.",
        }
    )
    log(f"Pushed to scan queue → job_id={jid2} (priority=DIRECTED, extended box)")
    return jid, jid2


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser(
        description="NWA Flight 2501 wreck hunt scan — CESAROPS/WreckHunter")
    ap.add_argument("--quick",        action="store_true",
                    help="Primary bbox only, 3 passes (stumpf, thermal, nir)")
    ap.add_argument("--extended",     action="store_true",
                    help="Full uncertainty box, all 5 passes")
    ap.add_argument("--no-download",  action="store_true",
                    help="Use locally cached tiles only, no HTTP downloads")
    ap.add_argument("--push-queue",   action="store_true",
                    help="Push jobs to scan_worker queue instead of running locally")
    args = ap.parse_args()

    if args.push_queue:
        j1, j2 = push_to_queue()
        print(f"\nQueued:\n  Primary  job_id={j1}\n  Extended job_id={j2}")
        print("\nCheck status:  python scan_queue.py list")
        return

    quick_passes = ["stumpf", "thermal", "nir"]
    full_passes  = ["stumpf", "thermal", "nir", "swir"]
    download     = not args.no_download

    results = []
    if args.extended:
        results.append(run_scan(NWA2501["extended_bbox"],
                                "extended", full_passes, download))
    else:
        # Always run primary
        passes = quick_passes if args.quick else full_passes
        results.append(run_scan(NWA2501["primary_bbox"],
                                "primary", passes, download))
        if not args.quick:
            results.append(run_scan(NWA2501["extended_bbox"],
                                    "extended", quick_passes, download))

    # Merge all KMZs into one combined file
    total_cands = sum(r["candidates"] for r in results)
    log(f"\n{'='*60}")
    log(f"NWA 2501 SCAN COMPLETE")
    log(f"  Total candidates: {total_cands}")
    log(f"  Output dir:       {OUTPUT_DIR}")
    log(f"{'='*60}\n")
    log("→ Load outputs/nwa2501/nwa2501_primary.kmz in Google Earth to review hits")
    log("→ Strongest hits are white/large icons — inspect with  nwa2501_candidates.json")


if __name__ == "__main__":
    main()
