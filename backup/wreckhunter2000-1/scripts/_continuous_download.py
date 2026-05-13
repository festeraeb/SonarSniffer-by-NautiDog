"""
Continuous HLS tile downloader — runs indefinitely, cycling through Great Lakes
lake regions and pulling the latest available tiles not yet on disk.

Run once in background:
    pythonw scripts\_continuous_download.py
    # or on Windows:
    Start-Process pythonw -ArgumentList "scripts\_continuous_download.py" -WindowStyle Hidden

Logs to: outputs/continuous_download.log
Writes PID to: outputs/continuous_download.pid
"""

import json
import os
import sys
import time
import logging
from pathlib import Path
from datetime import datetime, timedelta

import requests

# ── Paths ─────────────────────────────────────────────────────────────────────
ROOT     = Path(__file__).resolve().parent.parent
ENV_FILE = ROOT / ".env"
DOWNLOADS = ROOT / "downloads"
OUTPUTS   = ROOT / "outputs"
OUTPUTS.mkdir(exist_ok=True)
LOG_FILE  = OUTPUTS / "continuous_download.log"
PID_FILE  = OUTPUTS / "continuous_download.pid"

# Write our PID immediately so the user can kill us
PID_FILE.write_text(str(os.getpid()))

# ── Logging ───────────────────────────────────────────────────────────────────
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [DL] %(message)s",
    datefmt="%Y-%m-%d %H:%M:%S",
    handlers=[
        logging.FileHandler(LOG_FILE, encoding="utf-8"),
        logging.StreamHandler(sys.stdout),
    ],
)
log = logging.getLogger(__name__)

# ── Download targets ──────────────────────────────────────────────────────────
# (name, min_lon, min_lat, max_lon, max_lat, dest_subdir)
TARGETS = [
    # Known wreck clusters — highest priority
    ("superior_east",   -89.6, 46.4, -84.8, 47.6,  "superior_east/latest"),
    ("michigan_north",  -87.9, 45.7, -85.0, 47.5,  "michigan_north/latest"),
    ("michigan_south",  -88.0, 42.8, -85.0, 44.5,  "michigan_south/latest"),
    ("huron_north",     -85.0, 44.1, -82.3, 47.0,  "huron_north/latest"),
    ("erie_cluster",    -83.5, 42.1, -78.8, 43.5,  "erie_cluster/latest"),
    # Full lake sweeps for scan coverage
    ("lake_michigan",   -89.0, 41.5, -84.5, 47.0,  "michigan/latest"),
    ("lake_superior",   -92.2, 46.4, -84.4, 49.1,  "superior/latest"),
    ("lake_huron",      -84.7, 43.0, -79.7, 46.3,  "huron/latest"),
    ("lake_erie",       -83.5, 41.3, -78.8, 43.0,  "erie/latest"),
    ("lake_ontario",    -79.9, 43.1, -76.1, 44.3,  "ontario/latest"),
]

# How many granules to keep per target region
MAX_GRANULES_PER_TARGET = 3

# Only download cloud cover below this threshold (%) if available
MAX_CLOUD = 40.0

# Bands needed (these match wreck_ml_trainer.py)
KEY_BANDS = ["B02.tif", "B03.tif", "B04.tif", "B11.tif", "Fmask.tif"]

# How long each full cycle waits before repeating (in seconds)
CYCLE_SLEEP_SEC = 30 * 60   # 30 minutes

# ── Auth ──────────────────────────────────────────────────────────────────────
def load_env() -> dict:
    env = {}
    if ENV_FILE.exists():
        for line in ENV_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    return env


def make_session(token: str) -> requests.Session:
    s = requests.Session()
    s.headers.update({"Authorization": f"Bearer {token}", "Accept": "application/json"})
    return s


# ── CMR helpers ───────────────────────────────────────────────────────────────
def recent_temporal() -> str:
    """Last 30 days temporal window."""
    end   = datetime.utcnow()
    start = end - timedelta(days=30)
    return f"{start.strftime('%Y-%m-%dT%H:%M:%SZ')}/{end.strftime('%Y-%m-%dT%H:%M:%SZ')}"


def cmr_search_recent(session: requests.Session, bbox: tuple, product: str) -> list:
    min_lon, min_lat, max_lon, max_lat = bbox
    params = {
        "short_name": product,
        "bounding_box": f"{min_lon},{min_lat},{max_lon},{max_lat}",
        "temporal": recent_temporal(),
        "page_size": 50,
        "sort_key": "-start_date",
    }
    try:
        r = session.get(
            "https://cmr.earthdata.nasa.gov/search/granules.json",
            params=params, timeout=30,
        )
        if r.status_code == 200:
            return r.json().get("feed", {}).get("entry", [])
    except Exception as e:
        log.warning(f"CMR search error ({product}): {e}")
    return []


def pick_granules(granules: list, max_count: int) -> list:
    """Pick newest unique tile IDs, limited to max_count."""
    by_tile: dict = {}
    for g in granules:
        parts = g.get("title", "").split(".")
        tile_id = parts[2] if len(parts) >= 3 else g.get("title", "")
        ts = g.get("time_start", "")
        if tile_id not in by_tile or ts > by_tile[tile_id].get("time_start", ""):
            by_tile[tile_id] = g
    return sorted(by_tile.values(), key=lambda g: g.get("time_start", ""), reverse=True)[:max_count]


# ── Download ──────────────────────────────────────────────────────────────────
def download_granule(session: requests.Session, granule: dict, dest_dir: Path) -> int:
    title = granule.get("title", "unknown")
    gran_dir = dest_dir / title
    gran_dir.mkdir(parents=True, exist_ok=True)

    links = granule.get("links", [])
    seen_bands: set = set()
    target_links = []
    for lnk in links:
        href = lnk.get("href", "")
        if not href.endswith(".tif"):
            continue
        if not any(href.endswith(b) for b in KEY_BANDS):
            continue
        band = href.split("/")[-1]
        if band in seen_bands:
            continue
        seen_bands.add(band)
        target_links.append(href)

    if not target_links:
        return 0

    downloaded = 0
    for href in target_links:
        band = href.split("/")[-1]
        dest_file = gran_dir / band
        if dest_file.exists() and dest_file.stat().st_size > 10_000:
            continue
        try:
            r = session.get(href, stream=True, timeout=300)
            r.raise_for_status()
            with open(dest_file, "wb") as f:
                for chunk in r.iter_content(65536):
                    f.write(chunk)
            size_kb = dest_file.stat().st_size // 1024
            log.info(f"  + {band} ({size_kb} KB)")
            downloaded += 1
        except Exception as e:
            log.warning(f"  ! {band}: {e}")
        time.sleep(0.4)
    return downloaded


# ── Main loop ─────────────────────────────────────────────────────────────────
def run_cycle(session: requests.Session) -> int:
    total = 0
    for (name, min_lon, min_lat, max_lon, max_lat, subdir) in TARGETS:
        bbox = (min_lon, min_lat, max_lon, max_lat)
        dest_dir = DOWNLOADS / subdir
        log.info(f"--- {name} ---")

        all_granules = []
        for product in ["HLSL30", "HLSS30"]:
            found = cmr_search_recent(session, bbox, product)
            all_granules.extend(found)

        if not all_granules:
            log.info(f"  no recent granules for {name}")
            continue

        selected = pick_granules(all_granules, MAX_GRANULES_PER_TARGET)
        log.info(f"  {len(selected)} granule(s) to check")

        for g in selected:
            log.info(f"  -> {g.get('title','?')} ({g.get('time_start','?')[:10]})")
            n = download_granule(session, g, dest_dir)
            if n:
                log.info(f"     downloaded {n} bands")
            total += n
            time.sleep(1)

    return total


def main():
    log.info(f"=== Continuous downloader starting (PID {os.getpid()}) ===")
    env = load_env()
    token = env.get("EARTHDATA_TOKEN", "")
    if not token:
        log.error("EARTHDATA_TOKEN not found in .env — exiting")
        sys.exit(1)

    session = make_session(token)
    cycle_num = 0

    while True:
        cycle_num += 1
        log.info(f"\n=== CYCLE {cycle_num} start ===")
        try:
            n = run_cycle(session)
            log.info(f"=== CYCLE {cycle_num} done — {n} band files downloaded ===")
        except Exception as e:
            log.error(f"Cycle {cycle_num} error: {e}", exc_info=True)
        log.info(f"Sleeping {CYCLE_SLEEP_SEC // 60} min until next cycle...")
        time.sleep(CYCLE_SLEEP_SEC)


if __name__ == "__main__":
    main()
