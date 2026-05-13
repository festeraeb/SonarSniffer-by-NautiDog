#!/usr/bin/env python3
"""
Download ALL available satellite data for the Argo leak area (Kelley's Island, Erie)
Sept 15 - Oct 15, 2015

Sources:
- AWS STAC: Landsat 8 (optical + SWIR + thermal)
- AWS STAC: Sentinel-2 (if available - S2A launched June 2015)
- ASF: Sentinel-1 SAR (surface roughness - oil calms waves)
- AWS STAC: Landsat 5/7 for historical baseline
"""
import json
import os
import urllib.request
from pathlib import Path

OUTPUT = Path("/mnt/data-external/cesarops/repo/downloads/erie/2015/argo")
OUTPUT.mkdir(parents=True, exist_ok=True)

BBOX = [-82.8, 41.5, -82.4, 41.8]  # Kelley's Island area
STAC_URL = "https://earth-search.aws.element84.com/v1/search"
ASF_URL = "https://api.daac.asf.alaska.edu/services/search/param"

def search_stac(collection, start, end, limit=20):
    payload = json.dumps({
        "collections": [collection],
        "bbox": BBOX,
        "datetime": f"{start}T00:00:00Z/{end}T23:59:59Z",
        "limit": limit
    }).encode()
    req = urllib.request.Request(STAC_URL, data=payload, headers={"Content-Type": "application/json"})
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
        return resp.get("features", [])
    except Exception as e:
        print(f"  STAC error: {e}")
        return []

def search_asf(start, end):
    """Search ASF for Sentinel-1 SAR over the area."""
    center_lon = (BBOX[0] + BBOX[2]) / 2
    center_lat = (BBOX[1] + BBOX[3]) / 2
    params = {
        "platform": "Sentinel-1A",
        "processingLevel": "GRD_HD",
        "intersectsWith": f"POINT({center_lon} {center_lat})",
        "start": f"{start}T00:00:00UTC",
        "end": f"{end}T23:59:59UTC",
        "maxResults": 20,
        "output": "jsonlite",
    }
    url = f"{ASF_URL}?{'&'.join(f'{k}={v}' for k,v in params.items())}"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "CESAROPS"})
        resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
        if isinstance(resp, list):
            return resp
        return resp.get("results", [])
    except Exception as e:
        print(f"  ASF error: {e}")
        return []

def download_file(url, dest):
    if dest.exists():
        print(f"    exists: {dest.name}")
        return True
    try:
        urllib.request.urlretrieve(url, str(dest))
        print(f"    downloaded: {dest.name} ({dest.stat().st_size / 1e6:.1f} MB)")
        return True
    except Exception as e:
        print(f"    FAILED: {e}")
        return False

# === SEARCH ALL SOURCES ===
print("=" * 60)
print("ARGO LEAK AREA — Multi-Satellite Search")
print(f"BBOX: {BBOX}")
print(f"Window: 2015-09-15 to 2015-10-15")
print("=" * 60)

# 1. Landsat 8 (Collection 2 Level 2)
print("\n--- Landsat 8 (C2 L2) ---")
l8_scenes = search_stac("landsat-c2-l2", "2015-09-15", "2015-10-15")
print(f"Found: {len(l8_scenes)} scenes")
for f in l8_scenes:
    cloud = f["properties"].get("eo:cloud_cover", "?")
    print(f"  {f['id']} | {f['properties']['datetime'][:10]} | cloud: {cloud}%")

# 2. Landsat 5/7 (historical baseline - check 2010-2014 for comparison)
print("\n--- Landsat 5/7 (historical baseline 2014) ---")
l57_scenes = search_stac("landsat-c2-l2", "2014-08-01", "2014-10-31")
print(f"Found: {len(l57_scenes)} scenes (2014 baseline)")
for f in l57_scenes[:5]:
    print(f"  {f['id']} | {f['properties']['datetime'][:10]} | cloud: {f['properties'].get('eo:cloud_cover','?')}%")

# 3. Sentinel-2 (may not exist for 2015 - S2A was brand new)
print("\n--- Sentinel-2 (S2A launched June 2015) ---")
s2_scenes = search_stac("sentinel-2-l2a", "2015-09-15", "2015-10-15")
print(f"Found: {len(s2_scenes)} scenes")
for f in s2_scenes[:5]:
    print(f"  {f['id']} | {f['properties']['datetime'][:10]}")

# 4. Sentinel-1 SAR (ASF)
print("\n--- Sentinel-1 SAR (ASF) ---")
s1_scenes = search_asf("2015-09-15", "2015-10-15")
print(f"Found: {len(s1_scenes)} scenes")
for s in s1_scenes[:10]:
    name = s.get("granuleName", s.get("name", "?"))
    start_time = s.get("startTime", "?")
    print(f"  {name} | {start_time[:10]}")

# === DOWNLOAD KEY SCENES ===
print("\n" + "=" * 60)
print("DOWNLOADING KEY SCENES")
print("=" * 60)

TARGET_BANDS = ["blue", "green", "red", "nir08", "swir16", "swir22", "lwir11"]

for feat in l8_scenes:
    scene_id = feat["id"]
    # Only LC08 (Landsat 8), skip LE07 (Landsat 7 has scan line issues)
    if not scene_id.startswith("LC08"):
        continue
    cloud = feat["properties"].get("eo:cloud_cover", 100)
    if cloud > 30:
        print(f"\n  Skipping {scene_id} (cloud: {cloud}%)")
        continue

    print(f"\n  Downloading: {scene_id} (cloud: {cloud}%)")
    scene_dir = OUTPUT / scene_id
    scene_dir.mkdir(exist_ok=True)

    assets = feat.get("assets", {})
    for band in TARGET_BANDS:
        if band in assets:
            href = assets[band]["href"]
            if href.startswith("s3://usgs-landsat/"):
                href = href.replace("s3://usgs-landsat/", "https://landsatlook.usgs.gov/data/")
            dest = scene_dir / f"{band}.tif"
            download_file(href, dest)

print("\n\nDone.")
print(f"Output: {OUTPUT}")
os.system(f"du -sh {OUTPUT}/* 2>/dev/null")
