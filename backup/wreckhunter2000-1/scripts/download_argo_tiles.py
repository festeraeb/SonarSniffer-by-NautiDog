#!/usr/bin/env python3
"""Download the two key Landsat 8 scenes for Argo validation from AWS"""
import json
import os
import urllib.request
from pathlib import Path

OUTPUT = Path("/mnt/data-external/cesarops/repo/downloads/erie/2015/argo")
OUTPUT.mkdir(parents=True, exist_ok=True)

# Search STAC
payload = json.dumps({
    "collections": ["landsat-c2-l2"],
    "bbox": [-82.8, 41.5, -82.4, 41.8],
    "datetime": "2015-09-15T00:00:00Z/2015-10-15T00:00:00Z",
    "limit": 20
}).encode()

req = urllib.request.Request("https://earth-search.aws.element84.com/v1/search", data=payload, headers={"Content-Type": "application/json"})
resp = json.loads(urllib.request.urlopen(req, timeout=15).read())

# Download SWIR, blue, green, red, NIR for the two best LC08 scenes
TARGET_BANDS = ["blue", "green", "red", "nir08", "swir16", "swir22", "lwir11"]
TARGET_SCENES = ["LC08_L2SP_020031_20150921", "LC08_L2SP_020031_20151007"]

for feat in resp.get("features", []):
    scene_id = feat["id"]
    # Only download Landsat 8 (LC08) scenes
    if not any(t in scene_id for t in TARGET_SCENES):
        continue
    
    print(f"\n=== {scene_id} ===")
    print(f"  Date: {feat['properties']['datetime']}")
    print(f"  Cloud: {feat['properties'].get('eo:cloud_cover', '?')}%")
    
    assets = feat.get("assets", {})
    scene_dir = OUTPUT / scene_id
    scene_dir.mkdir(exist_ok=True)
    
    for band in TARGET_BANDS:
        if band in assets:
            href = assets[band]["href"]
            # Convert s3:// to https://
            if href.startswith("s3://usgs-landsat/"):
                href = href.replace("s3://usgs-landsat/", "https://landsatlook.usgs.gov/data/")
            
            dest = scene_dir / f"{scene_id}_{band}.tif"
            if dest.exists():
                print(f"  {band}: already exists")
                continue
            
            print(f"  {band}: downloading...")
            try:
                urllib.request.urlretrieve(href, str(dest))
                print(f"  {band}: {dest.stat().st_size / 1e6:.1f} MB")
            except Exception as e:
                print(f"  {band}: FAILED - {e}")

print("\nDone.")
print(f"Files in: {OUTPUT}")
os.system(f"du -sh {OUTPUT}/*")
