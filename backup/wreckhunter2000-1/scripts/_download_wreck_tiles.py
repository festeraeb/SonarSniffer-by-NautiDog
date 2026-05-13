"""
Download HLS tiles covering known wreck GPS coordinates so the ML trainer
can extract positive training patches.

Strategy:
  - Group the 87 known wrecks into lake clusters (Michigan, Huron, Erie, Superior)
  - Query CMR for HLSL30/HLSS30 granules intersecting each wreck cluster bbox
  - Pick the most recent cloud-free scene per tile ID (avoid duplicate granules)
  - Download bands: B02 (blue), B03 (green), B04 (red), B11 (SWIR), Fmask
  - Save to downloads/<lake>/wreck_tiles/<granule_title>/

Usage:
    python scripts/_download_wreck_tiles.py [--dry-run] [--lake michigan|huron|erie|superior|all]
"""

import argparse
import json
import sys
import time
from pathlib import Path
from collections import defaultdict

import requests

ROOT = Path(__file__).resolve().parent.parent
ENV_FILE = ROOT / ".env"
DOWNLOADS = ROOT / "downloads"
KNOWN_WRECKS = ROOT / "known_wrecks.json"

# Bands needed for patch extraction
KEY_BANDS = ["B02.tif", "B03.tif", "B04.tif", "B11.tif", "Fmask.tif"]

# How many tiles per cluster (to keep download size reasonable)
MAX_GRANULES_PER_CLUSTER = 6

# Temporal window — last 3 ice-free navigation seasons
TEMPORAL = "2022-06-01T00:00:00Z/2024-09-30T23:59:59Z"

# Wreck clusters: (name, min_lon, min_lat, max_lon, max_lat)
CLUSTERS = {
    "superior_east": (-89.6, 46.4, -84.8, 47.6),   # Keweenaw / eastern Superior
    "michigan_north": (-87.9, 45.7, -85.0, 47.5),  # N Michigan / Mackinac area
    "michigan_south": (-88.0, 42.8, -85.0, 44.5),  # Central/S Lake Michigan
    "huron_north":    (-85.0, 44.1, -82.3, 47.0),  # Lake Huron / Georgian Bay approaches
    "erie_cluster":   (-83.5, 42.1, -78.8, 43.5),  # Lake Erie wrecks
}


def load_env() -> dict:
    env = {}
    if ENV_FILE.exists():
        for line in ENV_FILE.read_text().splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    return env


def cmr_search(session: requests.Session, bbox: tuple, product: str = "HLSL30") -> list:
    """Search CMR for HLS granules in bbox. Returns list of granule dicts."""
    min_lon, min_lat, max_lon, max_lat = bbox
    params = {
        "short_name": product,
        "bounding_box": f"{min_lon},{min_lat},{max_lon},{max_lat}",
        "temporal": TEMPORAL,
        "page_size": 200,
        "sort_key": "-start_date",
    }
    granules = []
    page = 1
    while True:
        params["page_num"] = page
        r = session.get(
            "https://cmr.earthdata.nasa.gov/search/granules.json",
            params=params,
            timeout=30,
        )
        if r.status_code != 200:
            print(f"  [CMR {product}] HTTP {r.status_code}")
            break
        entries = r.json().get("feed", {}).get("entry", [])
        if not entries:
            break
        granules.extend(entries)
        if len(entries) < 200:
            break
        page += 1
    return granules


def pick_best_granules(granules: list, max_count: int) -> list:
    """
    From a list of CMR granules, pick up to max_count unique tile IDs,
    choosing the most recent date per tile.
    """
    by_tile: dict[str, dict] = {}
    for g in granules:
        title = g.get("title", "")
        parts = title.split(".")
        tile_id = parts[2] if len(parts) >= 3 else title
        existing = by_tile.get(tile_id)
        ts = g.get("time_start", "")
        if existing is None or ts > existing.get("time_start", ""):
            by_tile[tile_id] = g
    # Sort by tile ID for reproducibility, return up to max_count
    return list(sorted(by_tile.values(), key=lambda g: g.get("title", "")))[:max_count]


def download_granule(session: requests.Session, granule: dict, dest_dir: Path,
                     dry_run: bool = False) -> int:
    """Download key bands for one granule. Returns number of files downloaded."""
    title = granule.get("title", "unknown")
    links = [
        lnk for lnk in granule.get("links", [])
        if lnk.get("href", "").endswith(".tif")
    ]
    # Filter to key bands, deduplicate by filename
    seen_bands: set = set()
    target_links = []
    for lnk in links:
        if not any(lnk["href"].endswith(b) for b in KEY_BANDS):
            continue
        band = lnk["href"].split("/")[-1]
        if band in seen_bands:
            continue
        seen_bands.add(band)
        target_links.append(lnk)
    if not target_links:
        print(f"    [WARN] no matching band links for {title}")
        return 0

    gran_dir = dest_dir / title
    if not dry_run:
        gran_dir.mkdir(parents=True, exist_ok=True)

    downloaded = 0
    for lnk in target_links:
        href = lnk["href"]
        band = href.split("/")[-1]
        dest_file = gran_dir / band
        if not dry_run and dest_file.exists() and dest_file.stat().st_size > 10_000:
            print(f"      [skip] {band}")
            continue
        if dry_run:
            print(f"      [dry] {band} -> {gran_dir.relative_to(ROOT)}/{band}")
            downloaded += 1
            continue
        print(f"      {band}...", end=" ", flush=True)
        try:
            r = session.get(href, stream=True, timeout=300)
            r.raise_for_status()
            with open(dest_file, "wb") as f:
                for chunk in r.iter_content(65536):
                    f.write(chunk)
            size_kb = dest_file.stat().st_size // 1024
            print(f"{size_kb} KB")
            downloaded += 1
        except Exception as e:
            print(f"ERROR: {e}")
        time.sleep(0.3)  # be polite to LP DAAC
    return downloaded


def main():
    ap = argparse.ArgumentParser(description="Download HLS tiles covering known wreck GPS coords")
    ap.add_argument("--dry-run", action="store_true", help="Print what would be downloaded, no actual download")
    ap.add_argument("--lake", default="all", choices=["michigan", "huron", "erie", "superior", "all"],
                    help="Which lake cluster(s) to download (default: all)")
    ap.add_argument("--max-granules", type=int, default=MAX_GRANULES_PER_CLUSTER,
                    help=f"Max granules per cluster (default: {MAX_GRANULES_PER_CLUSTER})")
    args = ap.parse_args()

    env = load_env()
    token = env.get("EARTHDATA_TOKEN", "")
    if not token:
        print("[ERROR] EARTHDATA_TOKEN not found in .env — can't download")
        sys.exit(1)

    session = requests.Session()
    session.headers.update({
        "Authorization": f"Bearer {token}",
        "Accept": "application/json",
    })

    # Filter clusters by --lake flag
    lake_filter = args.lake
    cluster_map = {
        "michigan": ["michigan_north", "michigan_south"],
        "huron":    ["huron_north"],
        "erie":     ["erie_cluster"],
        "superior": ["superior_east"],
        "all":      list(CLUSTERS.keys()),
    }
    active_clusters = {k: v for k, v in CLUSTERS.items() if k in cluster_map[lake_filter]}

    total_downloaded = 0

    for cluster_name, bbox in active_clusters.items():
        dest_dir = DOWNLOADS / cluster_name / "wreck_tiles"
        print(f"\n{'='*60}")
        print(f"Cluster: {cluster_name}  bbox: {bbox}")
        print(f"Dest:    {dest_dir.relative_to(ROOT)}")

        all_granules = []
        for product in ["HLSL30", "HLSS30"]:
            found = cmr_search(session, bbox, product)
            print(f"  CMR {product}: {len(found)} granules found")
            all_granules.extend(found)

        if not all_granules:
            print("  [SKIP] No granules found for this cluster")
            continue

        selected = pick_best_granules(all_granules, args.max_granules)
        print(f"  Selected {len(selected)} unique tile IDs:")
        for g in selected:
            title = g.get("title", "?")
            date = g.get("time_start", "?")[:10]
            props = g.get("collection_concept_id", "")
            print(f"    {title}  ({date})")

        if args.dry_run:
            print("\n  --- DRY RUN: listing band links ---")

        for g in selected:
            print(f"\n  Downloading {g.get('title','?')} ...")
            n = download_granule(session, g, dest_dir, dry_run=args.dry_run)
            total_downloaded += n
            if not args.dry_run:
                time.sleep(1.0)

    print(f"\n{'='*60}")
    print(f"Done. {total_downloaded} band files {'(dry-run)' if args.dry_run else 'downloaded'}.")
    if not args.dry_run:
        print(f"Next: python wreck_ml_trainer.py --extract-only")


if __name__ == "__main__":
    main()
