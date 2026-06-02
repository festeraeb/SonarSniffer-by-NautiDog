"""
rossa_critical_download.py

Downloads Sentinel-2 L2A from CRITICAL Rossa sinking timeframe.

Sinking: Aug 22, 2025, 8-9 PM CDT

Priority downloads:
1. Aug 22, 2025 (day of sinking - BEFORE or DURING)
2. Aug 23-25, 2025 (1-3 days AFTER - freshest wreck signature)
3. Aug 26-29, 2025 (1 week after - debris still visible)
4. Sep 1-10, 2025 (2-3 weeks after - settling period)

Uses earth-search STAC (public AWS S3, no auth needed)
"""

import json
import requests
from datetime import datetime
from pathlib import Path

# ── Configuration ─────────────────────────────────────────────────────────────

# Search zone (8 miles out from McKinley)
SEARCH_ZONE = {
    'lat_min': 42.9,
    'lat_max': 43.2,
    'lon_min': -87.9,
    'lon_max': -87.5,
}

# Critical date ranges
DATE_RANGES = [
    {'name': 'DAY OF SINKING (BEFORE)', 'start': '2025-08-22', 'end': '2025-08-22', 'priority': 'CRITICAL'},
    {'name': '1-3 DAYS AFTER', 'start': '2025-08-23', 'end': '2025-08-25', 'priority': 'CRITICAL'},
    {'name': '1 WEEK AFTER', 'start': '2025-08-26', 'end': '2025-08-29', 'priority': 'HIGH'},
    {'name': '2-3 WEEKS AFTER', 'start': '2025-09-01', 'end': '2025-09-10', 'priority': 'MEDIUM'},
]

# earth-search STAC API
STAC_SEARCH = 'https://earth-search.aws.element84.com/v1/search'

# Output directory
OUTPUT_DIR = Path('c:/Users/thomf/programming/Bagrecovery/outputs/rossa_critical')
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ── Download Functions ────────────────────────────────────────────────────────

def query_sentinel2_stac(date_range: dict) -> list[dict]:
    """Query earth-search STAC for Sentinel-2 scenes."""
    
    payload = {
        'collections': ['sentinel-2-l2a'],
        'datetime': f"{date_range['start']}T00:00:00Z/{date_range['end']}T23:59:59Z",
        'intersects': {
            'type': 'Polygon',
            'coordinates': [[
                [SEARCH_ZONE['lon_min'], SEARCH_ZONE['lat_min']],
                [SEARCH_ZONE['lon_max'], SEARCH_ZONE['lat_min']],
                [SEARCH_ZONE['lon_max'], SEARCH_ZONE['lat_max']],
                [SEARCH_ZONE['lon_min'], SEARCH_ZONE['lat_max']],
                [SEARCH_ZONE['lon_min'], SEARCH_ZONE['lat_min']],
            ]]
        },
        'query': {
            'eo:cloud_cover': {'lte': 30},  # Max 30% cloud
        },
        'limit': 20,
        'sortby': [{'field': 'properties.datetime', 'direction': 'asc'}],
    }
    
    try:
        resp = requests.post(STAC_SEARCH, json=payload, timeout=60)
        resp.raise_for_status()
        
        features = resp.json().get('features', [])
        
        scenes = []
        for f in features:
            props = f.get('properties', {})
            scenes.append({
                'scene_id': f.get('id', ''),
                'datetime': props.get('datetime', ''),
                'cloud_cover': props.get('eo:cloud_cover', 100),
                'mgrs_tile': props.get('mgrs:grid_square', ''),
                'assets': f.get('assets', {}),
            })
        
        return scenes
        
    except Exception as e:
        print(f'  Query failed: {e}')
        return []


def download_scene_assets(scene: dict, bands: list[str], output_dir: Path) -> int:
    """Download specific bands from a Sentinel-2 scene."""
    
    downloaded = 0
    assets = scene.get('assets', {})
    
    for band in bands:
        # Find asset for this band
        asset_key = None
        for key in assets:
            if band.lower() in key.lower():
                asset_key = key
                break
        
        if not asset_key:
            continue
        
        asset_url = assets[asset_key].get('href', '')
        if not asset_url:
            continue
        
        # Convert S3 URL to HTTPS
        if asset_url.startswith('s3://'):
            asset_url = asset_url.replace('s3://sentinel-cogs/', 'https://sentinel-cogs.s3.us-west-2.amazonaws.com/')
        
        # Download
        filename = f"{scene['scene_id']}.{band}.tif"
        output_path = output_dir / filename
        
        if output_path.exists() and output_path.stat().st_size > 0:
            print(f'  ✓ {band}: Already downloaded')
            downloaded += 1
            continue
        
        try:
            print(f'  ↓ {band}: Downloading...')
            resp = requests.get(asset_url, timeout=300, stream=True)
            resp.raise_for_status()
            
            with open(output_path, 'wb') as f:
                for chunk in resp.iter_content(chunk_size=8192):
                    if chunk:
                        f.write(chunk)
            
            size_mb = output_path.stat().st_size / 1e6
            print(f'  ✓ {band}: Saved ({size_mb:.1f} MB)')
            downloaded += 1
            
        except Exception as e:
            print(f'  ✗ {band}: Failed - {e}')
    
    return downloaded


def main():
    """Download critical Rossa timeframe Sentinel-2 data."""
    
    print('='*70)
    print('ROSSA CRITICAL SATELLITE DOWNLOAD')
    print('='*70)
    print()
    print(f'Sinking: August 22, 2025, 8-9 PM CDT')
    print(f'Search Zone: {SEARCH_ZONE["lat_min"]}-{SEARCH_ZONE["lat_max"]}°N, {SEARCH_ZONE["lon_min"]}-{SEARCH_ZONE["lon_max"]}°W')
    print()
    
    # Bands to download (prioritize optical + red-edge for wreck detection)
    bands = ['B04', 'B03', 'B02', 'B08', 'B05', 'B11', 'B12']  # RGB, NIR, Red-Edge, SWIR
    
    total_downloaded = 0
    
    for date_range in DATE_RANGES:
        print(f"[{date_range['priority']}] {date_range['name']} ({date_range['start']} to {date_range['end']})")
        
        scenes = query_sentinel2_stac(date_range)
        
        if not scenes:
            print(f'  No scenes found')
            print()
            continue
        
        print(f'  Found {len(scenes)} scenes:')
        for i, scene in enumerate(scenes[:5], 1):
            print(f'    {i}. {scene["scene_id"][:50]}...')
            print(f'       Cloud: {scene["cloud_cover"]}%, Tile: {scene["mgrs_tile"]}')
        
        if len(scenes) > 5:
            print(f'    ... and {len(scenes) - 5} more')
        print()
        
        # Download assets from first 3 scenes (most critical)
        for scene in scenes[:3]:
            print(f'  Downloading {scene["scene_id"][:40]}...')
            downloaded = download_scene_assets(scene, bands, OUTPUT_DIR)
            total_downloaded += downloaded
            print()
    
    print('='*70)
    print(f'DOWNLOAD COMPLETE')
    print('='*70)
    print(f'Total bands downloaded: {total_downloaded}')
    print(f'Output: {OUTPUT_DIR}')
    print()
    print('Next: Run GPU curvelets on these scenes!')
    print('='*70)


if __name__ == '__main__':
    main()
