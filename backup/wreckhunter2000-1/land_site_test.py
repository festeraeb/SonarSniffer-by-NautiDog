#!/usr/bin/env python3
"""
CESAROPS Land-Site Aircraft Crash Test
=======================================
3-node pipeline for the 6 known aircraft crash sites:

  Pi  (100.127.66.32)  CONDUCTOR  — parallel HLS + USGS 3DEP-LiDAR CMR downloads
  M2200 (local)        GPU        — cupy SWIR bright-anomaly & thermal scan
  i7 TPU (10.0.0.56)  INFERENCE  — Coral TPU glint+jitter score per candidate patch

LiDAR where available: USGS 3DEP TNM DEM sampled at each detection point for
elevation and slope enrichment. LPC point clouds flagged if present.

Usage:
    python land_site_test.py                              # full pipeline
    python land_site_test.py --dry-run                    # Pi skipped; scan only cached files
    python land_site_test.py --skip-download              # assume downloads/jobs/ already populated
    python land_site_test.py --site t33_allenspark_co     # single site
    python land_site_test.py --site beechcraft18_mt_success_nh pa28_kaaterskill_high_peak_ny
"""

import argparse
import json
import math
import os
import stat as _stat
import sys
import time
from datetime import datetime
from pathlib import Path
from typing import Optional

if sys.platform == "win32":
    sys.stdout.reconfigure(encoding="utf-8")
    sys.stderr.reconfigure(encoding="utf-8")

# Block GDAL/PROJ from hitting the network — prevents hangs when rasterio
# opens local GeoTIFFs with HLS-style filenames while paramiko threads exist.
os.environ.setdefault("PROJ_NETWORK", "OFF")
os.environ.setdefault("GDAL_DISABLE_READDIR_ON_OPEN", "EMPTY_DIR")
os.environ.setdefault("CPL_VSIL_CURL_ALLOWED_EXTENSIONS", "")

REPO = Path(__file__).resolve().parent

# ── .env loader ───────────────────────────────────────────────────────────────

def _load_env(p: Path) -> dict:
    env: dict = {}
    if p.exists():
        for line in p.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    return env

_dotenv = _load_env(REPO / ".env")


def cfg(key: str, default: str = "") -> str:
    return os.environ.get(key, _dotenv.get(key, default))


# ── SAR-land sidecar config ───────────────────────────────────────────────────

_SAR_CFG       = json.loads((REPO / "sar_land_config.json").read_text(encoding="utf-8"))
_TERRAIN_MODES = _SAR_CFG["terrain_modes"]
_PATCH_CFG     = _SAR_CFG["training_patch"]
_RADIUS_CFG    = _SAR_CFG["detection_radius_m"]

# ── Cluster config ────────────────────────────────────────────────────────────

PI_HOST       = cfg("PI_TAILSCALE",  "100.127.66.32")
PI_USER       = cfg("PI_USER",       "pi")
PI_PASS       = cfg("PI_PASS",       "")
PI_KEY        = cfg("PI_KEY",        "")
PI_WORK       = cfg("PI_WORK",       "/mnt/pi-usb/cesarops/sync")
# Where universal_downloader.py + .env credentials live on Pi
PI_SCRIPT_DIR = cfg("PI_SCRIPT_DIR", "/home/pi/cesarops")
# Where Pi writes downloaded tiles (pull this back via SFTP)
PI_DL_ROOT    = cfg("PI_DL_ROOT",    "/home/pi/cesarops/downloads/jobs")

# Download window — wide enough to capture any available tile per site
DL_DATE_START = "2023-07-01"
DL_DATE_END   = datetime.now().strftime("%Y-%m-%d")

# ── Optional imports (all graceful) ──────────────────────────────────────────

try:
    import cupy as cp
    import numpy as np
    # Explicitly select device 0 (Quadro M2200) and CREATE the CUDA context
    # RIGHT NOW at import time — before paramiko/SFTP threads exist.
    # On Windows WDDM/Optimus, the first cuInit() can deadlock if other
    # threads are running when it tries to acquire the WDDM display lock.
    cp.cuda.Device(0).use()
    _cuda_boot = cp.zeros((4,), dtype=cp.float32)
    float(_cuda_boot.sum())   # forces cuInit + context creation + JIT
    del _cuda_boot
    _props = cp.cuda.runtime.getDeviceProperties(0)
    _GPU_NAME: str = (_props["name"].decode()
                      if isinstance(_props["name"], bytes)
                      else str(_props["name"]))
    _free_mb, _total_mb = (v // 1024 // 1024
                           for v in cp.cuda.Device(0).mem_info)
    HAS_GPU = True
except Exception as _cuda_ex:
    import numpy as cp          # type: ignore[assignment]
    import numpy as np
    HAS_GPU  = False
    _GPU_NAME = f"none ({_cuda_ex})"
    _free_mb = _total_mb = 0

try:
    import rasterio
    from rasterio.warp import transform as _rio_warp
    import rasterio.windows
    HAS_RASTERIO = True
except ImportError:
    HAS_RASTERIO = False

try:
    from PIL import Image as _PILImage
    HAS_PIL = True
except ImportError:
    HAS_PIL = False

try:
    from scipy.ndimage import (label as _ndimage_label,
                               center_of_mass as _ndimage_com,
                               maximum as _ndimage_max)
    HAS_SCIPY = True
except ImportError:
    HAS_SCIPY = False


# ── Site loader ───────────────────────────────────────────────────────────────

def load_land_sites(filter_ids: list = None) -> list:
    """Load all entries with environment='land' from known_wrecks.json."""
    data   = json.loads((REPO / "known_wrecks.json").read_text(encoding="utf-8"))
    wrecks = data.get("wrecks", {})
    sites  = []
    for wid, w in wrecks.items():
        if w.get("environment") != "land":
            continue
        if filter_ids and wid not in filter_ids:
            continue
        lat = w["lat"]
        lon = w["lon"]
        sites.append({
            "id":           wid,
            "name":         w.get("name", wid),
            "lat":          lat,
            "lon":          lon,
            "terrain_type": w.get("terrain_type", "forested_conifer"),
            "training_notes": w.get("training_notes", ""),
            # ±0.05° bbox (~5.5 km per side) — wide enough for 20m pixels + scatter
            "bbox": [
                round(lat - 0.05, 6),
                round(lon - 0.05, 6),
                round(lat + 0.05, 6),
                round(lon + 0.05, 6),
            ],
        })
    return sites


# ── Pi SSH / SFTP dispatch ────────────────────────────────────────────────────

def _pi_node():
    """Return a connected SSHNode for Pi (Tailscale), or None."""
    try:
        from remote_dispatch import SSHNode, HAS_PARAMIKO
    except ImportError:
        print("[PI] remote_dispatch not importable", flush=True)
        return None

    if not HAS_PARAMIKO:
        print("[PI] paramiko not installed — SSH dispatch unavailable", flush=True)
        return None

    node = SSHNode(host=PI_HOST, user=PI_USER, password=PI_PASS, key_path=PI_KEY)
    try:
        if node.ping():
            print(f"[PI] Connected → {PI_HOST}", flush=True)
            return node
        print(f"[PI] Ping failed ({PI_HOST}) — will use cached downloads", flush=True)
        return None
    except Exception as exc:
        print(f"[PI] Connection error: {exc}", flush=True)
        return None


def _pi_download_cmd(site: dict) -> str:
    """Shell fragment to run universal_downloader.py on Pi for one site.

    Credentials are forwarded from the laptop's .env so Pi doesn't need its own
    valid Earthdata token.  LiDAR is omitted here because the Pi's downloader
    version lacks that sensor; 3DEP fetch happens locally in process_site().
    """
    bb  = site["bbox"]
    sid = site["id"]
    out = f"{PI_DL_ROOT}/{sid}"

    # Forward only the Bearer token. See comments in creds block below.
    def _sq(v: str) -> str:
        return v.replace("'", "'\"'\"'")

    tk = _sq(cfg("EARTHDATA_TOKEN"))

    # Forward only the Bearer token — blanking USERNAME/PASSWORD prevents the
    # session's HTTPBasicAuth from overwriting the Bearer Authorization header,
    # which caused 401 on Pi. Bearer token alone is valid for CMR search + download.
    creds = (
        f"export EARTHDATA_USERNAME='' "
        f"EARTHDATA_PASSWORD='' "
        f"EARTHDATA_TOKEN='{tk}'"
    )

    return (
        f"mkdir -p {out} && "
        f"{creds} && "
        f"cd {PI_SCRIPT_DIR} && "
        f"python3 {PI_SCRIPT_DIR}/universal_downloader.py "
        f"--bbox {bb[0]},{bb[1]},{bb[2]},{bb[3]} "
        f"--dates {DL_DATE_START} {DL_DATE_END} "
        f"--sensors hls "
        f"--output {out} "
        f"--max-results 5"
    )


def _pi_tif_count(node, site_id: str) -> int:
    """Count .tif files Pi already has for site_id."""
    r = node.run(
        f"find {PI_DL_ROOT}/{site_id} -name '*.tif' 2>/dev/null | wc -l"
    )
    try:
        return int(r["stdout"].strip())
    except (ValueError, KeyError):
        return 0


def dispatch_downloads_to_pi(
    sites: list, node, min_tif: int = 100
) -> dict:
    """
    SSH into Pi, background all site downloads in parallel, then wait.
    Sites where Pi already has ≥min_tif .tif files are skipped.
    Returns {site_id: pi_remote_dir}.
    """
    need_dl = []
    for s in sites:
        count = _pi_tif_count(node, s["id"])
        if count >= min_tif:
            print(f"  [PI] {s['id']} — Pi has {count} .tif files, skip re-download",
                  flush=True)
        else:
            print(f"  [PI] {s['id']} — Pi has {count} .tif files, will download",
                  flush=True)
            need_dl.append(s)

    if not need_dl:
        print("[PI] All sites already downloaded on Pi.", flush=True)
        return {s["id"]: f"{PI_DL_ROOT}/{s['id']}" for s in sites}

    print(f"\n[PI] Launching {len(need_dl)} parallel downloads "
          f"(HLS + 3DEP-LiDAR) ...", flush=True)

    frags = []
    for s in need_dl:
        log = f"/tmp/dl_{s['id']}.log"
        frags.append(f"( {_pi_download_cmd(s)} > {log} 2>&1 ) &")

    frags.append("wait")
    frags.append("echo '[PI] All downloads finished'")

    # Print last 5 lines of each site log
    for s in need_dl:
        log = f"/tmp/dl_{s['id']}.log"
        frags.append(f"echo '--- {s['id']} ---' && tail -5 {log} 2>/dev/null || true")

    result = node.run("\n".join(frags), timeout=2400)   # 40 min max

    for line in result.get("stdout", "").splitlines():
        print(f"  [PI] {line}", flush=True)
    if result["exit_code"] != 0:
        print(f"[PI] Shell exited {result['exit_code']} (non-fatal)", flush=True)

    return {s["id"]: f"{PI_DL_ROOT}/{s['id']}" for s in sites}


# Bands needed for GPU glint scan + cloud masking;
# S30: B11=SWIR-1  B12=SWIR-2
# L30: B06=SWIR-1  B07=SWIR-2  B10/B11=thermal
# Both: Fmask (cloud mask, small)
_SFTP_BAND_KEEP = {".B06.", ".B07.", ".B10.", ".B11.", ".B12.", ".FMASK."}


def _sftp_want(filename: str) -> bool:
    """Return True for files to pull: sensor-appropriate SWIR/thermal/Fmask .tif or any .laz."""
    ext = filename.rsplit(".", 1)[-1].lower()
    if ext == "laz":
        return True
    if ext in ("tif", "tiff"):
        n = filename.upper()
        if ".FMASK." in n:
            return True
        if ".S30." in n:   # Sentinel-2: SWIR-1=B11, SWIR-2=B12
            return ".B11." in n or ".B12." in n
        if ".L30." in n:   # Landsat: SWIR-1=B06, SWIR-2=B07, thermal=B10/B11
            return any(b in n for b in (".B06.", ".B07.", ".B10.", ".B11."))
        # Unknown sensor: apply full keep list
        return any(b in n for b in _SFTP_BAND_KEEP)
    return False


def sftp_pull_site(node, pi_remote_dir: str, local_dir: Path) -> list:
    """
    SFTP-pull SWIR+thermal+Fmask .tif files and .laz files from Pi.
    Non-scan bands (B01-B05, B08, B09, B8A …) are skipped.
    Skips files already present with matching size.
    Returns list of local Paths pulled.
    """
    local_dir.mkdir(parents=True, exist_ok=True)
    sftp   = node._client.open_sftp()
    pulled = []

    def _recurse(remote: str, local: Path):
        try:
            attrs = sftp.listdir_attr(remote)
        except IOError:
            return
        for attr in attrs:
            rp = f"{remote}/{attr.filename}"
            lp = local / attr.filename
            if _stat.S_ISDIR(attr.st_mode):
                lp.mkdir(parents=True, exist_ok=True)
                _recurse(rp, lp)
            elif _sftp_want(attr.filename):
                if lp.exists() and lp.stat().st_size == attr.st_size:
                    pulled.append(lp)
                else:
                    size_mb = attr.st_size / 1e6
                    print(f"  [SFTP] ← {attr.filename} ({size_mb:.1f} MB)", flush=True)
                    sftp.get(rp, str(lp))
                    pulled.append(lp)

    _recurse(pi_remote_dir, local_dir)
    sftp.close()
    return pulled


# ── Band identification helpers ───────────────────────────────────────────────
# HLS.S30 (Sentinel-2): B11=SWIR-1 (1569nm), B12=SWIR-2 (2190nm)
# HLS.L30 (Landsat 8/9): B06=SWIR-1 (1566nm), B07=SWIR-2 (2200nm),
#                          B10/B11=thermal (10.9/12.0 µm) — NOT SWIR
# STAC fallbacks: asset names "swir16"/"swir22"

def _is_swir_b11(p: Path) -> bool:
    """SWIR-1 (~1.56 µm): S30.B11 or L30.B06; also STAC 'swir16' asset."""
    n = p.name.upper()
    if ".S30." in n and ".B11." in n:
        return True
    if ".L30." in n and ".B06." in n:
        return True
    return "SWIR16" in n or "SWIR1" in n    # STAC fallback (not L30/S30 prefix)

def _is_swir_b12(p: Path) -> bool:
    """SWIR-2 (~2.20 µm): S30.B12 or L30.B07; also STAC 'swir22' asset."""
    n = p.name.upper()
    if ".S30." in n and ".B12." in n:
        return True
    if ".L30." in n and ".B07." in n:
        return True
    return "SWIR22" in n or "SWIR2" in n    # STAC fallback

def _is_thermal(p: Path) -> bool:
    """Thermal IR: L30.B10 (10.9 µm) or L30.B11 (12.0 µm)."""
    n = p.name.upper()
    if ".L30." in n and (".B10." in n or ".B11." in n):
        return True
    return "ST_B10" in n or "LWIR" in n or "THERMAL" in n

def _is_dem(p: Path) -> bool:
    n = p.name.lower()
    return ("dem" in n or "dtm" in n) and p.suffix.lower() in (".tif", ".tiff")


# ── Z-threshold resolver ──────────────────────────────────────────────────────

def _resolve_z_threshold(site: dict) -> float:
    """
    Return SWIR B11 bright z-threshold for this site's terrain + current season.
    April 2026 → month=4 → leaf-off for deciduous sites.
    """
    terrain = site.get("terrain_type", "forested_conifer")
    mode    = _TERRAIN_MODES.get(terrain, {})
    month   = datetime.now().month

    leaf_off = mode.get("leaf_off_window", {})
    if leaf_off and month in leaf_off.get("months", []):
        return float(leaf_off.get("swir_z_threshold_override",
                                  mode.get("swir_z_threshold", 2.0)))
    return float(mode.get("swir_z_threshold",
                          _SAR_CFG["band_priority"]["primary"][0]["z_threshold"]))


# ── GPU (M2200) SWIR bright-anomaly scan ─────────────────────────────────────

def _zscore_gpu(data_2d):
    """
    Z-score a 2-D raster band on M2200 (cupy) or CPU fallback (numpy).
    Returns (z_array, (mean, std)) or (None, None) if not enough valid data.
    """
    x = cp.asarray(data_2d.astype("float32"))
    valid = x > 0
    n = int(cp.sum(valid))
    if n < 100:
        return None, None
    mean = float(cp.sum(x * valid) / n)
    std  = float(cp.sqrt(cp.sum(((x - mean) ** 2) * valid) / n))
    if std < 1e-6:
        return None, None
    z = ((x - mean) / std) * valid
    return z, (mean, std)


def _find_blobs(z_arr, threshold: float, min_pix: int = 4,
                max_blobs: int = 60) -> list:
    """
    Find connected bright blobs above threshold.
    Returns list of (centroid_row, centroid_col, n_pixels, max_z).
    """
    z_cpu = cp.asnumpy(z_arr) if HAS_GPU else np.array(z_arr)
    mask  = (z_cpu > threshold).astype(np.uint8)

    if HAS_SCIPY:
        labeled, n_blobs = _ndimage_label(mask)
        blobs = []
        if n_blobs > 0:
            # O(n_pixels) blob sizes — fast bincount instead of per-blob argwhere
            counts = np.bincount(labeled.ravel(), minlength=n_blobs + 1)[1:]
            valid_mask = counts >= min_pix
            if valid_mask.any():
                ids = np.where(valid_mask)[0] + 1   # 1-indexed label IDs
                ids_list = ids.tolist()
                # Vectorised scipy calls — O(n_pixels) each regardless of n_blobs
                coms  = _ndimage_com(mask, labeled, ids_list)
                maxzs = _ndimage_max(z_cpu, labeled, ids_list)
                if n_blobs == 1:  # scipy returns scalar instead of list for single id
                    coms  = [coms]
                    maxzs = [maxzs]
                for (cr, cc), mz, vid in zip(coms, maxzs, ids_list):
                    blobs.append((int(round(cr)), int(round(cc)),
                                  int(counts[vid - 1]), float(mz)))
    else:
        # Fallback: treat every hot pixel as its own blob
        hot   = np.argwhere(mask == 1)
        blobs = [(int(r), int(c), 1, float(z_cpu[r, c])) for r, c in hot]

    blobs.sort(key=lambda b: -b[3])
    return blobs[:max_blobs]


def _row_col_to_latlon(src, row: int, col: int):
    """Convert raster row/col to WGS84 (lat, lon) pair."""
    x, y = rasterio.transform.xy(src.transform, row, col)
    lon_arr, lat_arr = _rio_warp(src.crs, "EPSG:4326", [x], [y])
    return float(lat_arr[0]), float(lon_arr[0])


def gpu_scan_tif(tif_path: Path, site: dict, z_thresh: float) -> list:
    """
    Run M2200 cupy zscore scan on a single-band TIF (B11 or B12).
    Returns list of raw detection dicts (patch stored as numpy array).
    """
    if not HAS_RASTERIO:
        print("  [GPU] rasterio unavailable — skipping tile", flush=True)
        return []

    with rasterio.open(tif_path) as src:
        data = src.read(1)
        z, stats = _zscore_gpu(data)
        if z is None:
            return []

        blobs = _find_blobs(z, z_thresh)
        detections = []
        half = _PATCH_CFG.get("patch_half_default", 12)

        # Use smaller patch for intact fuselage sites
        notes = site.get("training_notes", "").lower()
        if "intact" in notes and "scatter" not in notes:
            half = _PATCH_CFG.get("patch_half_intact_fuselage", 10)
        elif "scatter" in notes:
            half = _PATCH_CFG.get("patch_half_scattered_debris", 20)

        for cr, cc, npix, maxz in blobs:
            r0 = max(0, cr - half);  r1 = min(data.shape[0], cr + half)
            c0 = max(0, cc - half);  c1 = min(data.shape[1], cc + half)
            patch = data[r0:r1, c0:c1].astype("float32")

            lat, lon = _row_col_to_latlon(src, cr, cc)

            detections.append({
                "site_id":  site["id"],
                "band":     tif_path.name,
                "row":      cr,
                "col":      cc,
                "lat":      round(lat, 6),
                "lon":      round(lon, 6),
                "max_z":    round(maxz, 3),
                "n_pixels": npix,
                "terrain":  site["terrain_type"],
                "_patch":   patch,          # numpy array; stripped before JSON output
            })

        return detections


# ── USGS 3DEP DEM enrichment ─────────────────────────────────────────────────

def lidar_elevation_at(lidar_dir: Path, lat: float, lon: float) -> Optional[dict]:
    """
    Sample the first DEM .tif in lidar_dir that covers (lat, lon).
    Returns {source, elevation_m, slope_deg} or None.
    """
    if not HAS_RASTERIO:
        return None

    dems = [p for p in lidar_dir.rglob("*.tif") if _is_dem(p)]
    dems += [p for p in lidar_dir.rglob("*.tiff") if _is_dem(p)]

    for dem_path in dems:
        try:
            with rasterio.open(dem_path) as src:
                xs, ys = _rio_warp("EPSG:4326", src.crs, [lon], [lat])
                try:
                    row, col = src.index(xs[0], ys[0])
                except Exception:
                    continue
                if not (1 <= row < src.height - 1 and 1 <= col < src.width - 1):
                    continue

                # Read 3×3 neighbourhood for slope
                win = rasterio.windows.Window(col - 1, row - 1, 3, 3)
                hood = src.read(1, window=win)
                if hood.shape != (3, 3):
                    continue

                elev = float(hood[1, 1])
                if elev < -9000:
                    continue

                res_m = abs(src.transform.a)
                dz_dx = (hood[1, 2] - hood[1, 0]) / (2 * res_m)
                dz_dy = (hood[0, 1] - hood[2, 1]) / (2 * res_m)
                slope  = round(math.degrees(math.atan(math.sqrt(dz_dx**2 + dz_dy**2))), 1)

                return {
                    "source":       dem_path.name,
                    "elevation_m":  round(elev, 1),
                    "slope_deg":    slope,
                }
        except Exception:
            continue

    return None


def lidar_lpc_available(lidar_dir: Path) -> list:
    """Return list of .laz filenames present (point cloud inventory only)."""
    return [p.name for p in lidar_dir.rglob("*.laz")]


# ── Coral TPU scoring (i7) ────────────────────────────────────────────────────

def _patch_to_uint8(patch: np.ndarray) -> Optional[np.ndarray]:
    p  = patch.astype("float32")
    mn, mx = p.min(), p.max()
    if mx - mn < 1e-6:
        return None
    return ((p - mn) / (mx - mn) * 255).astype("uint8")


def tpu_score_detections(detections: list, tpu) -> list:
    """
    Send each detection's patch to the Coral TPU (or CPU stub) for glint scoring.
    Pops the '_patch' key and adds TPU result fields.
    """
    for det in detections:
        patch = det.pop("_patch", None)

        if patch is None or not HAS_PIL:
            det.update(glint_score=None, jitter_score=None, tpu_pass=None,
                       tpu_ms=None, used_tpu=False)
            continue

        arr8 = _patch_to_uint8(patch)
        if arr8 is None:
            det.update(glint_score=None, jitter_score=None, tpu_pass=None,
                       tpu_ms=None, used_tpu=False)
            continue

        img    = _PILImage.fromarray(arr8, mode="L")
        result = tpu.check_glint_jitter(
            img,
            meta={"site_id": det["site_id"], "band": det["band"],
                  "terrain": det.get("terrain", "")},
        )

        det["glint_score"]  = round(result.get("glint_score",  0.0), 4)
        det["jitter_score"] = round(result.get("jitter_score", 0.0), 4)
        det["tpu_pass"]     = result.get("pass", False)
        det["tpu_ms"]       = result.get("took_ms")
        det["used_tpu"]     = result.get("used_tpu", False)

    return detections


# ── Per-site pipeline ─────────────────────────────────────────────────────────

def process_site(site: dict, download_dir: Path, tpu) -> dict:
    """
    For one site: find SWIR tiles → M2200 GPU scan → LiDAR enrich → TPU score.
    """
    print(f"\n{'─' * 64}", flush=True)
    print(f"[SITE] {site['id']}", flush=True)
    print(f"       name={site['name']}", flush=True)
    print(f"       lat={site['lat']}  lon={site['lon']}", flush=True)
    print(f"       terrain={site['terrain_type']}", flush=True)

    result = {
        "site_id":     site["id"],
        "name":        site["name"],
        "lat":         site["lat"],
        "lon":         site["lon"],
        "terrain":     site["terrain_type"],
        "tiles_found": 0,
        "swir_tiles":  0,
        "detections":  [],
        "site_lidar":  None,
        "lpc_files":   [],
    }

    if not download_dir.exists():
        print(f"  [SITE] No tiles at {download_dir} — check Pi download", flush=True)
        return result

    all_tifs   = list(download_dir.rglob("*.tif")) + list(download_dir.rglob("*.tiff"))
    swir_tifs  = [t for t in all_tifs if _is_swir_b11(t) or _is_swir_b12(t)]
    therm_tifs = [t for t in all_tifs if _is_thermal(t)]

    result["tiles_found"] = len(all_tifs)
    result["swir_tiles"]  = len(swir_tifs)

    print(f"  [SITE] {len(all_tifs)} total TIFs | "
          f"{len(swir_tifs)} SWIR | {len(therm_tifs)} thermal", flush=True)

    z_thresh = _resolve_z_threshold(site)
    print(f"  [SITE] Z-threshold: {z_thresh}  "
          f"(terrain={site['terrain_type']}, month={datetime.now().month})", flush=True)

    all_dets: list = []

    # ── GPU scan B11/B12 SWIR bands ────────────────────────────────────────
    for tif in swir_tifs:
        band_tag = "B11" if _is_swir_b11(tif) else "B12"
        print(f"  [GPU] {band_tag}  {tif.name}", flush=True)
        dets = gpu_scan_tif(tif, site, z_thresh)
        if dets:
            print(f"  [GPU] → {len(dets)} candidates above z={z_thresh}", flush=True)
        all_dets.extend(dets)

    if not all_dets:
        print("  [GPU] No SWIR detections — tile may not cover site or cloud masked",
              flush=True)

    # ── LiDAR DEM enrichment ───────────────────────────────────────────────
    lidar_dir = download_dir / "lidar"
    if lidar_dir.exists():
        # Inventory LPC point clouds
        lpc = lidar_lpc_available(lidar_dir)
        if lpc:
            print(f"  [LIDAR] {len(lpc)} LPC point cloud(s): {lpc}", flush=True)
            result["lpc_files"] = lpc

        # Sample DEM at site centre
        elev = lidar_elevation_at(lidar_dir, site["lat"], site["lon"])
        if elev:
            print(f"  [LIDAR] Site DEM: "
                  f"elev={elev['elevation_m']} m  slope={elev['slope_deg']}°  "
                  f"({elev['source']})", flush=True)
            result["site_lidar"] = elev
        else:
            print("  [LIDAR] No DEM coverage at site coords", flush=True)

        # Enrich each detection with its local DEM value
        for det in all_dets:
            ei = lidar_elevation_at(lidar_dir, det["lat"], det["lon"])
            if ei:
                det["lidar"] = ei
    else:
        print(f"  [LIDAR] No LiDAR data for {site['id']} "
              f"(dir not present: {lidar_dir})", flush=True)

    # ── TPU scoring ───────────────────────────────────────────────────────
    if all_dets:
        print(f"  [TPU] Scoring {len(all_dets)} candidates ...", flush=True)
        all_dets = tpu_score_detections(all_dets, tpu)
        passed   = sum(1 for d in all_dets if d.get("tpu_pass"))
        print(f"  [TPU] {passed}/{len(all_dets)} passed glint+jitter check", flush=True)

    result["detections"] = all_dets
    return result


# ── Main orchestrator ─────────────────────────────────────────────────────────

def run_land_site_test(filter_ids: list = None,
                       dry_run: bool   = False,
                       skip_dl: bool   = False) -> dict:

    sites = load_land_sites(filter_ids)
    if not sites:
        print("No matching land sites found.", flush=True)
        return {}

    print("=" * 70, flush=True)
    print("CESAROPS LAND-SITE AIRCRAFT CRASH TEST", flush=True)
    print(f"Date   : {datetime.now():%Y-%m-%d %H:%M}", flush=True)
    print(f"Sites  : {len(sites)}", flush=True)
    if HAS_GPU:
        print(f"GPU    : {_GPU_NAME}  ({_free_mb} MB free / {_total_mb} MB total)", flush=True)
    else:
        print(f"GPU    : CPU numpy fallback  [{_GPU_NAME}]", flush=True)
    print(f"Rasterio: {'yes' if HAS_RASTERIO else 'NOT INSTALLED'}", flush=True)
    print("=" * 70, flush=True)

    for s in sites:
        print(f"  {s['id']:40s}  terrain={s['terrain_type']}", flush=True)

    # ── Step 1: TPU connection ────────────────────────────────────────────
    print("\n[INIT] Connecting to i7 Coral TPU server ...", flush=True)
    from tpu_client import create_tpu_client
    tpu    = create_tpu_client()
    health = tpu.health_check()
    print(f"[TPU] endpoint={tpu.server_url}  "
          f"status={health.get('status', '?')}  "
          f"tpu_hw={health.get('tpu_available', False)}  "
          f"model={health.get('model_loaded', False)}", flush=True)

    # ── Step 2: Pi downloads OR use cached ───────────────────────────────
    site_dirs: dict = {}

    if skip_dl or dry_run:
        print("\n[DOWNLOAD] Skipped (--skip-download / --dry-run) — "
              "using downloads/jobs/ cache", flush=True)
        for s in sites:
            site_dirs[s["id"]] = REPO / "downloads" / "jobs" / s["id"]

    else:
        pi = _pi_node()

        if pi:
            # Dispatch all parallel downloads to Pi
            pi_dirs = dispatch_downloads_to_pi(sites, pi)

            # SFTP pull each site's tiles back to laptop
            print("\n[SFTP] Pulling tiles from Pi ...", flush=True)
            for s in sites:
                ld = REPO / "downloads" / "jobs" / s["id"]
                print(f"  [SFTP] {s['id']} ← {pi_dirs[s['id']]}", flush=True)
                try:
                    pulled = sftp_pull_site(pi, pi_dirs[s["id"]], ld)
                    print(f"  [SFTP] {len(pulled)} files", flush=True)
                except Exception as exc:
                    print(f"  [SFTP] {s['id']}: {exc} — using cache", flush=True)
                site_dirs[s["id"]] = ld

            pi.close()

        else:
            print("[PI] Unreachable — using cached downloads/jobs/", flush=True)
            for s in sites:
                site_dirs[s["id"]] = REPO / "downloads" / "jobs" / s["id"]

    # ── Step 3: Per-site GPU + LiDAR + TPU ───────────────────────────────
    all_results = []
    for s in sites:
        dl_dir = site_dirs.get(s["id"], REPO / "downloads" / "jobs" / s["id"])
        res    = process_site(s, dl_dir, tpu)
        all_results.append(res)

    # ── Step 4: Summary ───────────────────────────────────────────────────
    total_dets  = sum(len(r["detections"]) for r in all_results)
    tpu_passed  = sum(
        sum(1 for d in r["detections"] if d.get("tpu_pass"))
        for r in all_results
    )
    with_lidar  = sum(1 for r in all_results if r.get("site_lidar"))
    with_lpc    = sum(1 for r in all_results if r.get("lpc_files"))

    print("\n" + "=" * 70, flush=True)
    print("LAND-SITE TEST SUMMARY", flush=True)
    print(f"  Sites tested      : {len(all_results)}", flush=True)
    print(f"  Total detections  : {total_dets}", flush=True)
    print(f"  TPU passed        : {tpu_passed}", flush=True)
    print(f"  Sites w/ DEM      : {with_lidar}", flush=True)
    print(f"  Sites w/ LPC      : {with_lpc}", flush=True)
    print(f"  GPU backend       : {'cupy M2200' if HAS_GPU else 'numpy CPU'}", flush=True)
    print("=" * 70, flush=True)

    report = {
        "run_time":         datetime.now().isoformat(),
        "gpu_mode":         "cupy_m2200" if HAS_GPU else "cpu_fallback",
        "sites_tested":     len(all_results),
        "total_detections": total_dets,
        "tpu_passed":       tpu_passed,
        "sites_with_dem":   with_lidar,
        "sites_with_lpc":   with_lpc,
        "results":          all_results,
    }

    (REPO / "outputs").mkdir(exist_ok=True)
    ts       = datetime.now().strftime("%Y%m%d_%H%M%S")
    out_path = REPO / "outputs" / f"land_site_test_{ts}.json"
    out_path.write_text(json.dumps(report, indent=2, default=str), encoding="utf-8")
    print(f"\n[REPORT] {out_path}", flush=True)

    kml_path = REPO / "outputs" / f"land_site_test_{ts}.kml"
    write_kml(report, kml_path)
    print(f"[KML]    {kml_path}", flush=True)

    return report


# ── KML export ────────────────────────────────────────────────────────────────

def _kml_style_id(max_z: float) -> str:
    """Map z-score to style id."""
    if max_z >= 15:
        return "sHigh"
    if max_z >= 8:
        return "sMed"
    return "sLow"


def write_kml(report: dict, out_path: Path) -> None:
    """Write a Google Earth KML from a land_site_test report dict.

    One Folder per site.  One Placemark per TPU-passed detection.
    A special star Placemark marks each site's claimed crash lat/lon.

    Color scheme:
      Red    (sHigh) — max_z >= 15
      Orange (sMed)  — max_z 8..14.9
      Yellow (sLow)  — max_z < 8
    """

    def _e(s: str) -> str:
        """Escape XML special chars."""
        return (str(s)
                .replace("&", "&amp;")
                .replace("<", "&lt;")
                .replace(">", "&gt;")
                .replace('"', "&quot;"))

    def _band_date(band: str) -> str:
        """Extract human date from HLS band filename, e.g. 2023184 → 2023-Jul-03."""
        import re
        m = re.search(r"\.(\d{7})T", band)
        if not m:
            return band
        raw = m.group(1)
        try:
            from datetime import datetime as _dt
            return _dt.strptime(raw, "%Y%j").strftime("%Y-%m-%d")
        except Exception:
            return raw

    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<kml xmlns="http://www.opengis.net/kml/2.2">',
        '<Document>',
        f'  <name>Land-Site Crash Detections — {report.get("run_time","")[:10]}</name>',
        '  <description>CESAROPS GPU/TPU SWIR anomaly scan — 6 land crash sites</description>',
        '',
        '  <!-- ── Styles ─────────────────────────────────────────────── -->',
        '  <Style id="sHigh">',
        '    <IconStyle><color>ff1400ff</color><scale>0.9</scale>',
        '      <Icon><href>http://maps.google.com/mapfiles/kml/shapes/star.png</href></Icon>',
        '    </IconStyle>',
        '    <LabelStyle><scale>0</scale></LabelStyle>',
        '  </Style>',
        '  <Style id="sMed">',
        '    <IconStyle><color>ff0078ff</color><scale>0.7</scale>',
        '      <Icon><href>http://maps.google.com/mapfiles/kml/shapes/donut.png</href></Icon>',
        '    </IconStyle>',
        '    <LabelStyle><scale>0</scale></LabelStyle>',
        '  </Style>',
        '  <Style id="sLow">',
        '    <IconStyle><color>ff00d7ff</color><scale>0.55</scale>',
        '      <Icon><href>http://maps.google.com/mapfiles/kml/shapes/placemark_circle.png</href></Icon>',
        '    </IconStyle>',
        '    <LabelStyle><scale>0</scale></LabelStyle>',
        '  </Style>',
        '  <Style id="sSite">',
        '    <IconStyle><color>ffff0000</color><scale>1.4</scale>',
        '      <Icon><href>http://maps.google.com/mapfiles/kml/shapes/target.png</href></Icon>',
        '    </IconStyle>',
        '  </Style>',
        '',
    ]

    for site_rec in report.get("results", []):
        site_id   = site_rec.get("site_id", "unknown")
        site_name = site_rec.get("name", site_id)
        site_lat  = site_rec.get("lat", 0.0)
        site_lon  = site_rec.get("lon", 0.0)
        terrain   = site_rec.get("terrain", "")
        detections = site_rec.get("detections", [])
        passed = [d for d in detections if d.get("tpu_pass")]

        lines += [
            f'  <Folder>',
            f'    <name>{_e(site_name)}</name>',
            f'    <description>terrain={_e(terrain)} | detections={len(passed)} tpu-passed</description>',
            '',
            # Site epicenter placemark
            f'    <Placemark>',
            f'      <name>★ {_e(site_name)}</name>',
            f'      <styleUrl>#sSite</styleUrl>',
            f'      <description>Claimed crash site | terrain={_e(terrain)}</description>',
            f'      <Point><coordinates>{site_lon},{site_lat},0</coordinates></Point>',
            f'    </Placemark>',
        ]

        for d in passed:
            det_lat   = d.get("lat", 0.0)
            det_lon   = d.get("lon", 0.0)
            max_z     = d.get("max_z", 0.0)
            n_pix     = d.get("n_pixels", 0)
            band      = d.get("band", "")
            band_date = _band_date(band)
            style_id  = _kml_style_id(max_z)
            dist_m    = _haversine_m(site_lat, site_lon, det_lat, det_lon)

            desc = (
                f"Date: {_e(band_date)}<br/>"
                f"Band: {_e(band)}<br/>"
                f"max_z: {max_z:.2f}<br/>"
                f"pixels: {n_pix}<br/>"
                f"terrain: {_e(terrain)}<br/>"
                f"dist_from_site: {dist_m:.0f} m<br/>"
                f"glint: {d.get('glint_score',0):.3f}  jitter: {d.get('jitter_score',0):.3f}"
            )
            pname = f"z={max_z:.1f} {band_date}"
            lines += [
                f'    <Placemark>',
                f'      <name>{_e(pname)}</name>',
                f'      <styleUrl>#{style_id}</styleUrl>',
                f'      <description>{desc}</description>',
                f'      <Point><coordinates>{det_lon},{det_lat},0</coordinates></Point>',
                f'    </Placemark>',
            ]

        lines += ['  </Folder>', '']

    lines += ['</Document>', '</kml>']

    out_path.write_text("\n".join(lines), encoding="utf-8")


def _haversine_m(lat1, lon1, lat2, lon2) -> float:
    """Great-circle distance in metres."""
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi / 2) ** 2 + math.cos(phi1) * math.cos(phi2) * math.sin(dlam / 2) ** 2
    return 2 * R * math.asin(math.sqrt(a))


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    p = argparse.ArgumentParser(
        description="CESAROPS Land-Site Aircraft Crash Test — Pi + M2200 + TPU"
    )
    p.add_argument(
        "--dry-run", action="store_true",
        help="Skip Pi SSH and SFTP; use whatever is already in downloads/jobs/",
    )
    p.add_argument(
        "--skip-download", action="store_true",
        help="Same as --dry-run but more explicit: Pi dispatch is skipped",
    )
    p.add_argument(
        "--site", nargs="+", metavar="SITE_ID",
        help="Limit test to one or more site IDs (e.g. t33_allenspark_co)",
    )
    args = p.parse_args()

    run_land_site_test(
        filter_ids=args.site,
        dry_run=args.dry_run,
        skip_dl=args.skip_download or args.dry_run,
    )


if __name__ == "__main__":
    main()
