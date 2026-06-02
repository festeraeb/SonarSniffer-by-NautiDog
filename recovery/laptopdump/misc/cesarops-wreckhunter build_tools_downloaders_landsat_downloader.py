"""Minimal Landsat downloader wrapper.

This is a best-effort helper: it will try to use `landsatxplore` if installed
and will otherwise print instructions for manual download. It expects the
caller to supply the bbox (minx,miny,maxx,maxy) and output directory.

Note: Actual automated downloads may require USGS credentials or API tokens.
"""
from pathlib import Path
import sys

def download_landsat_for_bbox(bbox, out_dir: Path, start_date=None, end_date=None):
    out_dir = Path(out_dir)
    out_dir.mkdir(parents=True, exist_ok=True)
    try:
        from landsatxplore.api import API
        from landsatxplore.earthexplorer import EarthExplorer
    except Exception:
        print("landsatxplore not available. Install with: pip install landsatxplore")
        print("Or manually download from USGS EarthExplorer or AWS Public datasets.")
        return

    # This helper requires the user to provide USGS credentials via env vars or prompt.
    import os
    user = os.environ.get("USGS_USER")
    password = os.environ.get("USGS_PASS")
    if not user or not password:
        print("USGS credentials not found in env (USGS_USER / USGS_PASS). Aborting automated download.")
        return

    api = API(user, password)
    scenes = api.search(dataset='LANDSAT_8_C1', bbox=bbox, start_date=start_date or '2010-01-01', end_date=end_date or '2026-01-01')
    print(f"Found {len(scenes)} scenes (first 5 shown):", scenes[:5])

    # Download up to 5 scenes as a demo
    ee = EarthExplorer(user, password)
    count = 0
    for s in scenes:
        scene_id = s['entityId'] if 'entityId' in s else s.get('displayId', None)
        if not scene_id:
            continue
        print("Downloading", scene_id)
        ee.download(scene_id, output_dir=str(out_dir))
        count += 1
        if count >= 5:
            break

    ee.logout()
    api.logout()
