#!/usr/bin/env python3
"""
CESAROPS Mission Runner
=======================
Generic front-end for every mission in missions.py.

By default runs locally — uses your GPU (cupy) if available, falls back to numpy.
With --queue the job is pushed to the scan_worker queue instead (i7 picks it up).

Usage
-----
  python run_mission.py --list
  python run_mission.py hormuz_mines
  python run_mission.py hormuz_mines --local           # force local  (default)
  python run_mission.py hormuz_mines --queue           # push to i7 queue
  python run_mission.py hormuz_mines --both            # local AND queue
  python run_mission.py nome_cessna  --zone primary_35nm
  python run_mission.py nwa2501      --extended        # use extended bbox
  python run_mission.py --all --queue                  # queue every mission

Environment variables (from .env):
  EARTHDATA_TOKEN   — NASA CMR bearer token (required for tile download)
  WORKER_MIN_FREE_RAM_GB — OOM guard (default 2.0)
"""

from __future__ import annotations

import argparse
import json
import logging
import os
import sys
import time
import warnings
from datetime import datetime, timezone
from pathlib import Path
from typing import Any

# ── Logging ───────────────────────────────────────────────────────────────────

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    handlers=[logging.StreamHandler(sys.stdout)],
)
log = logging.getLogger("run_mission")

REPO = Path(__file__).resolve().parent

# ── .env loader ───────────────────────────────────────────────────────────────

def _load_env() -> dict:
    env: dict[str, str] = {}
    p = REPO / ".env"
    if p.exists():
        for line in p.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    return env

_DOTENV = _load_env()
def _cfg(key: str, default: str = "") -> str:
    return os.environ.get(key, _DOTENV.get(key, default))

EARTHDATA_TOKEN = _cfg("EARTHDATA_TOKEN")
MIN_FREE_GB     = float(_cfg("WORKER_MIN_FREE_RAM_GB", "2.0"))


# ── GPU / array backend ───────────────────────────────────────────────────────

def _init_xp():
    """
    Return (xp, gpu_label).
    Tries cupy first (M2200 or any CUDA GPU), falls back to numpy.

    To enable GPU on this machine (Quadro M2200, driver 582.41, CUDA 13.2):
        pip uninstall cupy-cuda11x -y
        pip install cupy-cuda12x
      then install CUDA Toolkit 12.x from https://developer.nvidia.com/cuda-downloads
      (the toolkit creates cudart64_12*.dll — the stubs in Program Files/NVIDIA GPU Computing Toolkit are empty)
    """
    import numpy as _np
    try:
        import cupy as cp
        # Force a real CUDA call to confirm DLLs are loadable
        dev = cp.cuda.Device(0)
        dev.use()
        props = cp.cuda.runtime.getDeviceProperties(dev.id)
        name = props["name"].decode() if isinstance(props["name"], bytes) else props["name"]
        mem_mb = props["totalGlobalMem"] // 1024 // 1024
        log.info(f"GPU: {name}  {mem_mb} MiB  (cupy {cp.__version__})")
        return cp, f"{name} ({mem_mb}MiB, cupy)"
    except ImportError:
        log.info("cupy not installed — using numpy (CPU). "
                 "To enable GPU: pip install cupy-cuda12x")
    except Exception as e:
        log.info(
            f"cupy loaded but CUDA runtime unavailable ({type(e).__name__}: {e})\n"
            "  Fix: install CUDA Toolkit 12.x from https://developer.nvidia.com/cuda-downloads\n"
            "       then: pip uninstall cupy-cuda11x -y && pip install cupy-cuda12x\n"
            "  Falling back to numpy (CPU) — full pixel analysis still works."
        )
    return _np, "numpy (CPU)"

xp, GPU_LABEL = _init_xp()


# ── CMR helpers ───────────────────────────────────────────────────────────────

CMR_BASE = "https://cmr.earthdata.nasa.gov/search/granules.json"

def _cmr_query(concept_id: str, bbox: list[float],
               date_start: str, date_end: str,
               cloud_max: float = 100.0,
               page_size: int = 50) -> list[dict]:
    """
    Query NASA CMR for granules. bbox = [lat_min, lon_min, lat_max, lon_max]
    Returns list of {granule_ur, cloud_cover, download_url, date}.
    """
    try:
        import requests
    except ImportError:
        log.error("requests not installed — pip install requests")
        return []

    lat_min, lon_min, lat_max, lon_max = bbox
    params = {
        "short_name": "",
        "concept_id": concept_id,
        "bounding_box": f"{lon_min},{lat_min},{lon_max},{lat_max}",
        "temporal": f"{date_start}T00:00:00Z,{date_end}T23:59:59Z",
        "page_size": page_size,
        "sort_key[]": "-start_date",
    }
    if cloud_max < 100.0:
        params["cloud_cover[max]"] = int(cloud_max)

    headers = {}
    if EARTHDATA_TOKEN:
        headers["Authorization"] = f"Bearer {EARTHDATA_TOKEN}"

    granules: list[dict] = []
    page = 1
    while True:
        params["page_num"] = page
        try:
            r = requests.get(CMR_BASE, params=params, headers=headers, timeout=30)
            r.raise_for_status()
        except Exception as e:
            log.error(f"CMR query failed: {e}")
            break

        hits = r.json().get("feed", {}).get("entry", [])
        if not hits:
            break

        for g in hits:
            links = g.get("links", [])
            dl_url = next(
                (lk["href"] for lk in links
                 if lk.get("rel", "").endswith("/data#") and
                    lk.get("href", "").startswith("https")),
                None,
            )
            granules.append({
                "id":      g.get("id", ""),
                "title":   g.get("title", ""),
                "date":    g.get("time_start", "")[:10],
                "cloud":   float(g.get("cloud_cover", 0) or 0),
                "url":     dl_url,
            })

        if len(hits) < page_size:
            break
        page += 1

    return granules


# ── Tile downloader ───────────────────────────────────────────────────────────

def _download_tile(url: str, dest: Path) -> Path | None:
    """Download a single granule. Returns path or None on failure."""
    if dest.exists():
        return dest
    if not EARTHDATA_TOKEN:
        log.warning("EARTHDATA_TOKEN not set — skipping download")
        return None
    try:
        import requests
        headers = {"Authorization": f"Bearer {EARTHDATA_TOKEN}"}
        log.info(f"Downloading {dest.name} ...")
        with requests.get(url, headers=headers, stream=True, timeout=120) as r:
            r.raise_for_status()
            dest.parent.mkdir(parents=True, exist_ok=True)
            with dest.open("wb") as fh:
                for chunk in r.iter_content(chunk_size=1 << 20):
                    fh.write(chunk)
        return dest
    except Exception as e:
        log.error(f"Download failed for {url}: {e}")
        return None


# ── Spectral pass functions ───────────────────────────────────────────────────
# All use `xp` (cupy or numpy) for GPU-accelerated array ops.
# Pass functions return list of {label, lat, lon, score, note} candidates.

def _load_band(path: Path, band: int = 1):
    """Load a raster band → xp array (GPU if cupy, else numpy)."""
    try:
        import rasterio
        with rasterio.open(str(path)) as ds:
            arr = ds.read(band).astype("float32")
            transform = ds.transform
            crs = ds.crs
        return xp.asarray(arr), transform, crs
    except Exception as e:
        log.error(f"Failed to load {path.name} band {band}: {e}")
        return None, None, None


def _local_zscore(arr, window: int = 20):
    """GPU-accelerated local z-score using uniform filter."""
    try:
        if hasattr(xp, 'ElementwiseKernel'):
            # cupy path — use raw scipy-like rolling stats
            from cupyx.scipy.ndimage import uniform_filter
        else:
            from scipy.ndimage import uniform_filter
        local_mean = xp.asarray(uniform_filter(xp.asnumpy(arr) if hasattr(arr, 'get') else arr,
                                               size=window, mode='reflect').astype('float32'))
        local_std  = xp.asarray(
            uniform_filter(
                ((xp.asnumpy(arr) if hasattr(arr, 'get') else arr) ** 2),
                size=window, mode='reflect'
            ).astype('float32')
        )
        local_std = xp.sqrt(xp.maximum(local_std - local_mean ** 2, 1e-9))
        return (arr - local_mean) / local_std
    except Exception:
        # Pure fallback
        mean = xp.mean(arr)
        std  = xp.std(arr) + 1e-9
        return (arr - mean) / std


def _arr_to_candidates(zscore_arr, transform, crs, z_thresh: float,
                       label_prefix: str, max_per_tile: int = 30) -> list[dict]:
    """Convert z-score array → lat/lon candidate list."""
    try:
        import numpy as np
        z_np = xp.asnumpy(zscore_arr) if hasattr(zscore_arr, 'get') else zscore_arr
        ys, xs = (z_np > z_thresh).nonzero()
        if len(ys) == 0:
            return []
        scores = z_np[ys, xs]
        # Top N by score
        idx = scores.argsort()[::-1][:max_per_tile]
        ys, xs, scores = ys[idx], xs[idx], scores[idx]
        candidates = []
        for y, x, s in zip(ys, xs, scores):
            lon, lat = transform * (float(x), float(y))
            candidates.append({
                "label": f"{label_prefix} z={s:.2f}",
                "lat":   round(lat, 6),
                "lon":   round(lon, 6),
                "score": round(float(s), 3),
            })
        return candidates
    except Exception as e:
        log.error(f"arr_to_candidates error: {e}")
        return []


def pass_sar_bright(tile: Path, thresh: dict) -> list[dict]:
    """SAR bright point anomaly — high-backscatter discrete target on uniform bg."""
    arr, tf, crs = _load_band(tile, band=1)
    if arr is None:
        return []
    z = _local_zscore(arr, window=thresh.get("window", 20))
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 3.0), "SAR_BRIGHT")


def pass_sar_change(tile_before: Path, tile_after: Path, thresh: dict) -> list[dict]:
    """SAR temporal change — new high-backscatter returns vs previous pass."""
    before, tf, crs = _load_band(tile_before, band=1)
    after,  _,  _   = _load_band(tile_after, band=1)
    if before is None or after is None:
        return []
    diff = after - before
    diff_pos = xp.maximum(diff, 0)  # only new returns, not disappearing ones
    z = _local_zscore(diff_pos)
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 2.5), "SAR_CHANGE")


def pass_ice_fracture(tile: Path, thresh: dict) -> list[dict]:
    """
    SAR ice impact fracture — linear low-backscatter streak (open water channel
    through ice from impact direction). Detected as strong negative z in uniform ice.
    """
    arr, tf, crs = _load_band(tile, band=1)
    if arr is None:
        return []
    z = _local_zscore(arr, window=15)
    z_neg = -z  # invert: looking for low-return streak
    return _arr_to_candidates(z_neg, tf, crs, thresh.get("z_thresh", 2.5), "ICE_FRACTURE")


def pass_turbidity(tile: Path, thresh: dict) -> list[dict]:
    """Turbidity anomaly using B02/B03 ratio."""
    b02, tf, crs = _load_band(tile, band=1)   # assumes pre-stacked tile
    b03, _,  _   = _load_band(tile, band=2)
    if b02 is None or b03 is None:
        return []
    ratio = b02 / (b03 + 1e-6)
    z = _local_zscore(ratio)
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 2.0), "TURBIDITY")


def pass_optical_surf(tile: Path, thresh: dict) -> list[dict]:
    """High surface reflectance in B02 above water mask."""
    b02, tf, crs = _load_band(tile, band=1)
    if b02 is None:
        return []
    refl = b02 / 10000.0   # HLS scale factor
    hits = (refl > thresh.get("reflectance_thresh", 0.15)).astype("float32")
    z = hits * 4.0   # binary mask — scale to fake z for candidate extractor
    return _arr_to_candidates(xp.asarray(z) if not hasattr(z, 'get') else z,
                              tf, crs, 3.5, "OPTICAL_SURF")


def pass_stumpf(tile: Path, thresh: dict) -> list[dict]:
    """Stumpf ratio B02/B03 log — shallow submerged structure detection."""
    b02, tf, crs = _load_band(tile, band=1)
    b03, _,  _   = _load_band(tile, band=2)
    if b02 is None or b03 is None:
        return []
    ratio = xp.log(b02 / (b03 + 1e-6) + 1e-9)
    z = _local_zscore(ratio)
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 1.8), "STUMPF")


def pass_thermal_cold(tile: Path, thresh: dict) -> list[dict]:
    """Cold spot anomaly — submerged metal conducts cold bottom water."""
    arr, tf, crs = _load_band(tile, band=1)
    if arr is None:
        return []
    z = _local_zscore(arr)
    z_inv = -z   # cold = negative z in thermal band
    return _arr_to_candidates(z_inv, tf, crs, thresh.get("z_thresh", 1.5), "THERMAL_COLD")


def pass_nir_anomaly(tile: Path, thresh: dict) -> list[dict]:
    arr, tf, crs = _load_band(tile, band=1)
    if arr is None:
        return []
    z = _local_zscore(arr)
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 2.0), "NIR_ANOM")


def pass_swir_fuel(tile: Path, thresh: dict) -> list[dict]:
    arr, tf, crs = _load_band(tile, band=1)
    if arr is None:
        return []
    z = _local_zscore(arr)
    return _arr_to_candidates(z, tf, crs, thresh.get("z_thresh", 2.2), "SWIR_FUEL")


PASS_FN = {
    "sar_bright":   pass_sar_bright,
    "sar_change":   pass_sar_change,
    "ice_fracture": pass_ice_fracture,
    "turbidity":    pass_turbidity,
    "optical_surf": pass_optical_surf,
    "stumpf":       pass_stumpf,
    "thermal_cold": pass_thermal_cold,
    "nir_anomaly":  pass_nir_anomaly,
    "swir_fuel":    pass_swir_fuel,
}


# ── KMZ builder ───────────────────────────────────────────────────────────────

def _risk_color(score: float, n_passes: int) -> str:
    if n_passes >= 3 or score > 4.0:
        return "ff0000ff"   # RED   — CRITICAL
    if n_passes == 2 or score > 3.0:
        return "ff0080ff"   # ORANGE — HIGH
    if n_passes == 1 or score > 2.0:
        return "ff00ffff"   # YELLOW — MEDIUM
    return "ff00ff80"       # GREEN  — LOW


def _write_kmz(candidates: list[dict], mission: dict, out_dir: Path) -> Path:
    """Write candidates to KMZ."""
    import zipfile, io
    out_dir.mkdir(parents=True, exist_ok=True)
    ts  = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
    kmz = out_dir / f"{mission['output_dir'].replace('/','_')}_{ts}.kmz"

    placemarks = []
    for c in candidates:
        color = _risk_color(c["score"], c.get("n_passes", 1))
        placemarks.append(f"""
    <Placemark>
      <name>{c['label']}</name>
      <description>{c.get('note', '')}  score={c['score']:.2f}</description>
      <Style><IconStyle><color>{color}</color><scale>1.2</scale>
        <Icon><href>http://maps.google.com/mapfiles/kml/shapes/target.png</href></Icon>
      </IconStyle></Style>
      <Point><coordinates>{c['lon']},{c['lat']},0</coordinates></Point>
    </Placemark>""")

    # Historical risk zones
    for rz in mission.get("historical_zones", []):
        placemarks.append(f"""
    <Placemark>
      <name>[HIST] {rz['label']}</name>
      <description>{rz.get('note', '')} ({rz['risk']})</description>
      <Style><IconStyle><color>ffff8800</color><scale>1.0</scale></IconStyle></Style>
      <Point><coordinates>{rz['lon']},{rz['lat']},0</coordinates></Point>
    </Placemark>""")

    kml = f"""<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>{mission['label']}</name>
  <description>GPU: {GPU_LABEL}  |  Run: {ts}  |  Candidates: {len(candidates)}</description>
  {''.join(placemarks)}
</Document>
</kml>"""

    with zipfile.ZipFile(str(kmz), "w", zipfile.ZIP_DEFLATED) as zf:
        zf.writestr("doc.kml", kml)

    log.info(f"KMZ written: {kmz.relative_to(REPO)}  ({len(candidates)} candidates)")
    return kmz


# ── Queue push ────────────────────────────────────────────────────────────────

def _push_to_queue(mission: dict, bbox: list[float], sensors: list[str],
                   job_label: str, params: dict = None) -> str:
    import scan_queue as Q
    m_params = {
        "date_start":    mission["date_range"][0],
        "date_end":      mission["date_range"][1],
        "mission_name":  mission.get("output_dir", ""),
        "target_type":   mission["target_type"],
        **(params or {}),
    }
    job_id = Q.push(
        label    = job_label,
        bbox     = bbox,
        sensors  = sensors,
        priority = mission.get("priority", Q.PRIORITY_DIRECTED),
        params   = m_params,
    )
    log.info(f"Queued: {job_label}  id={job_id}  priority={mission.get('priority',1)}")
    return job_id


# ── Local mission runner ───────────────────────────────────────────────────────

def _bbox_for(mission: dict, zone_key: str | None, use_extended: bool) -> list[float]:
    """Resolve which bbox to use."""
    if zone_key is not None:
        zones = mission.get("sub_zones", {})
        if zone_key in zones:
            return zones[zone_key]
        log.warning(f"Zone '{zone_key}' not in mission — using primary bbox")
    if use_extended:
        ext = (mission.get("sub_zones") or {}).get("extended")
        if ext:
            return ext
    return mission["bbox"]


def _run_local(mission: dict, bbox: list[float], args: argparse.Namespace):
    """
    Full local run:
      1. CMR granule discovery
      2. Download tiles (if EARTHDATA_TOKEN set)
      3. Run spectral passes on each tile
      4. Merge + deduplicate candidates
      5. Write KMZ
    """
    t0 = time.time()
    m_name = mission["output_dir"].replace("outputs/", "").replace("/", "_")
    log.info(f"=== LOCAL RUN: {mission['label']} ===")
    log.info(f"GPU backend: {GPU_LABEL}")
    log.info(f"Bbox: {bbox}")
    log.info(f"Dates: {mission['date_range'][0]} → {mission['date_range'][1]}")

    tile_root = REPO / "downloads" / m_name
    out_root  = REPO / "outputs" / m_name
    tile_root.mkdir(parents=True, exist_ok=True)

    thresholds = mission.get("thresholds", {})
    cloud_max  = thresholds.get("max_cloud", 60.0)

    # 1. CMR granule discovery for each sensor
    all_granules: list[dict] = []
    sensors_cfg = mission.get("sensors", {})
    for sensor_key, concept_id in sensors_cfg.items():
        log.info(f"CMR query: {sensor_key} = {concept_id}")
        g = _cmr_query(
            concept_id  = concept_id,
            bbox        = bbox,
            date_start  = mission["date_range"][0],
            date_end    = mission["date_range"][1],
            cloud_max   = cloud_max,
        )
        log.info(f"  → {len(g)} granules")
        for gr in g:
            gr["sensor_key"] = sensor_key
        all_granules.extend(g)

    if not all_granules:
        log.warning("No granules found — check dates, bbox, and CMR concept IDs")
        return []

    # 2. Download tiles
    downloaded: list[Path] = []
    if not EARTHDATA_TOKEN:
        log.warning("EARTHDATA_TOKEN not set — pixel passes skipped (granule count only)")
    else:
        for g in all_granules[:args.max_tiles]:
            if not g["url"]:
                continue
            fname = Path(g["url"]).name
            dest  = tile_root / fname
            p = _download_tile(g["url"], dest)
            if p:
                downloaded.append(p)

    # 3. Run passes on downloaded tiles
    all_candidates: list[dict] = []
    passes = mission.get("passes", [])

    if downloaded:
        log.info(f"Running {len(passes)} pass(es) on {len(downloaded)} tile(s) with {GPU_LABEL}")
        # For sar_change we need tile pairs — group by sensor, sort by date
        sar_tiles = [p for p in downloaded if p.suffix == ".tif"]

        for tile in sar_tiles:
            g_meta = next((g for g in all_granules if Path(g["url"]).name == tile.name), {})
            for pass_name in passes:
                if pass_name == "sar_change":
                    continue   # handled separately below
                fn = PASS_FN.get(pass_name)
                if fn is None:
                    log.warning(f"Pass '{pass_name}' not implemented — skipping")
                    continue
                thresh = thresholds.get(pass_name, {})
                if isinstance(thresh, (int, float)):
                    thresh = {"z_thresh": thresh}
                try:
                    hits = fn(tile, thresh)
                    for h in hits:
                        h["pass"]  = pass_name
                        h["tile"]  = tile.name
                        h["date"]  = g_meta.get("date", "")
                    all_candidates.extend(hits)
                except Exception as e:
                    log.error(f"Pass {pass_name} failed on {tile.name}: {e}")

        # SAR temporal change — need consecutive pairs
        if "sar_change" in passes and len(sar_tiles) >= 2:
            thresh = thresholds.get("sar_change", {})
            if isinstance(thresh, (int, float)):
                thresh = {"z_thresh": thresh}
            sar_tiles_sorted = sorted(sar_tiles,
                                      key=lambda p: p.stat().st_mtime)
            for i in range(len(sar_tiles_sorted) - 1):
                try:
                    hits = pass_sar_change(sar_tiles_sorted[i],
                                           sar_tiles_sorted[i + 1], thresh)
                    for h in hits:
                        h["pass"] = "sar_change"
                    all_candidates.extend(hits)
                except Exception as e:
                    log.error(f"sar_change pair {i}: {e}")
    else:
        log.info("No tiles downloaded — reporting granule count and historical zones only")

    # 4. Merge historical risk zones as low-confidence baseline candidates
    for rz in mission.get("historical_zones", []):
        all_candidates.append({
            "label":   f"[HISTORICAL] {rz['label']}",
            "lat":     rz["lat"],
            "lon":     rz["lon"],
            "score":   1.0,
            "note":    rz.get("note", ""),
            "pass":    "historical",
            "risk":    rz.get("risk", "LOW"),
            "n_passes": 1,
        })

    # 5. Summarise
    log.info(f"=== {mission['label']} — {len(all_candidates)} candidates ===")
    by_pass: dict[str, int] = {}
    for c in all_candidates:
        by_pass[c.get("pass", "?")] = by_pass.get(c.get("pass", "?"), 0) + 1
    for p, n in sorted(by_pass.items()):
        log.info(f"  {p:<20} {n:>4} candidates")

    # 6. Save JSON
    out_root.mkdir(parents=True, exist_ok=True)
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
    json_path = out_root / f"{m_name}_{ts}.json"
    json_path.write_text(json.dumps({
        "mission":    mission["label"],
        "gpu":        GPU_LABEL,
        "bbox":       bbox,
        "dates":      mission["date_range"],
        "granules":   len(all_granules),
        "tiles":      len(downloaded),
        "candidates": all_candidates,
    }, indent=2), encoding="utf-8")
    log.info(f"Results: {json_path.relative_to(REPO)}")

    # 7. KMZ
    _write_kmz(all_candidates, mission, out_root)

    log.info(f"Elapsed: {time.time() - t0:.1f}s")
    return all_candidates


# ── CLI ───────────────────────────────────────────────────────────────────────

def _parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(
        prog="run_mission",
        description="CESAROPS mission runner — GPU-local or queue push",
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    p.add_argument("mission", nargs="?", help="Mission name (see --list)")
    p.add_argument("--list",     action="store_true", help="Show all missions")
    p.add_argument("--all",      action="store_true", help="Run / queue all missions")

    # Execution mode
    mode = p.add_mutually_exclusive_group()
    mode.add_argument("--local",  action="store_true", default=True,
                      help="Run locally (GPU first, CPU fallback) [DEFAULT]")
    mode.add_argument("--queue",  action="store_true",
                      help="Push jobs to i7 scan queue instead of running locally")
    mode.add_argument("--both",   action="store_true",
                      help="Run locally AND push to queue")

    # Target selection
    p.add_argument("--zone",     metavar="ZONE_KEY",
                   help="Run a specific sub-zone (e.g. primary_35nm, bottleneck)")
    p.add_argument("--extended", action="store_true",
                   help="Use extended bbox if mission defines one")

    # Limits
    p.add_argument("--max-tiles", type=int, default=20,
                   help="Max tiles to download locally (default 20)")
    p.add_argument("--dry-run",   action="store_true",
                   help="CMR query and plan only, no downloads or pixel passes")

    return p.parse_args()


def main():
    import missions as M

    args = _parse_args()

    # --queue or --both clears the default --local=True
    run_local = not args.queue   # True unless --queue only
    push_queue = args.queue or args.both

    if args.list:
        M.list_missions()
        sys.exit(0)

    mission_names: list[str] = []
    if args.all:
        mission_names = list(M.MISSIONS.keys())
    elif args.mission:
        mission_names = [args.mission]
    else:
        print("Usage: python run_mission.py <mission_name>  or  --list")
        sys.exit(1)

    for m_name in mission_names:
        try:
            mission = M.get(m_name)
        except KeyError as e:
            log.error(str(e))
            continue

        bbox = _bbox_for(mission, args.zone, args.extended)

        # ── Queue push ────────────────────────────────────────────────────
        if push_queue:
            jobs = mission.get("queue_jobs")
            if jobs:
                sub_zones = mission.get("sub_zones", {})
                sensors_cfg = mission.get("sensors", {})
                sensors_all = list(sensors_cfg.keys())
                for job in jobs:
                    bk = job.get("bbox_key", "")
                    jbox = sub_zones.get(bk, mission["bbox"])
                    jsens = job.get("sensors", sensors_all)
                    _push_to_queue(mission, jbox, jsens, job["label"])
            else:
                # Single job for whole bbox
                _push_to_queue(mission, bbox,
                               list(mission.get("sensors", {}).keys()),
                               m_name)

        # ── Local GPU run ─────────────────────────────────────────────────
        if run_local and not args.dry_run:
            _run_local(mission, bbox, args)
        elif args.dry_run:
            log.info(f"[DRY RUN] {m_name}: bbox={bbox}  "
                     f"dates={mission['date_range']}  "
                     f"passes={mission.get('passes')}  "
                     f"sensors={list(mission.get('sensors', {}).keys())}")


if __name__ == "__main__":
    main()
