"""
icesat2_batch_downloader.py

Batch downloads ICESat-2 ATL13 (laser altimetry) granules for Lake Michigan.

Requirements for best wreck detection:
  - LOW TURBIDITY (clear water for laser penetration)
  - CALM SURFACE (low wind/waves)
  - LOW WATER LEVELS (exposes more wreck structure)

Note: ICESat-2 launched October 2018, so NO 2012 data available.
For 2012 low water, use Landsat 5/7 thermal instead.
"""

import json
import os
import time
from datetime import datetime
from pathlib import Path
import requests

# ── Configuration ─────────────────────────────────────────────────────────────

# Lake Michigan bounding box
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9,
    'lat_min': 41.5,
    'lon_max': -85.5,
    'lat_max': 46.0,
}

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'icesat2_atl13'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Earthdata token
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

# NASA CMR API
CMR_BASE = 'https://cmr.earthdata.nasa.gov/search/granules.json'
NSIDC_DOWNLOAD = 'https://n5eil01u.ecs.nsidc.org/DP7/ATL13.005/'

# ICESat-2 product
ICESAT2_PRODUCT = 'ATL13'  # Along-Track Height

# ── Helpers ───────────────────────────────────────────────────────────────────

def load_earthdata_token() -> str:
    """Load Earthdata token."""
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    return json.loads(tp.read_text(encoding='utf-8')).get('earthdata_token', '')
                else:
                    return tp.read_text(encoding='utf-8').strip()
            except Exception:
                continue
    return ''


def query_icesat2_granules(
    bbox: dict,
    date_range: tuple[str, str],
    token: str,
    max_results: int = 200,
) -> list[dict]:
    """
    Query NASA CMR for ICESat-2 ATL13 granules.
    
    Note: ICESat-2 has very sparse coverage (91-day repeat cycle, narrow track).
    Most dates will return 0 granules.
    
    Returns list of granule metadata with download URLs.
    """
    headers = {
        'Accept': 'application/json',
        'Authorization': f'Bearer {token}',
    }
    
    params = {
        'short_name': ICESAT2_PRODUCT,
        'temporal': f'{date_range[0]}T00:00:00Z,{date_range[1]}T23:59:59Z',
        'bounding_box': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'page_size': min(max_results, 2000),
    }
    
    print(f'Querying CMR for ICESat-2 ATL13 ({date_range[0]} to {date_range[1]})...')
    print(f'  Note: ICESat-2 has 91-day repeat cycle, narrow ground track')
    print(f'  Most dates will have NO coverage over Lake Michigan')
    
    try:
        resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=120)
        resp.raise_for_status()
        
        entries = resp.json().get('feed', {}).get('entry', [])
        
        granules = []
        for entry in entries:
            # Extract download URL
            links = entry.get('links', [])
            dl_url = next(
                (l['href'] for l in links
                 if l.get('rel') == 'http://esipfed.org/ns/fedsearch/1.1/data#'
                 and 'https://' in l.get('href', '')),
                None
            )
            
            if dl_url:
                granules.append({
                    'granule_id': entry.get('id', ''),
                    'title': entry.get('title', ''),
                    'time_start': entry.get('time_start', ''),
                    'time_end': entry.get('time_end', ''),
                    'dl_url': dl_url,
                })
        
        if len(granules) > 0:
            print(f'  ✓ Found {len(granules)} granules with download URLs')
        else:
            print(f'  ⚠ No granules found (sparse coverage is normal)')
        
        return granules
        
    except Exception as e:
        print(f'  CMR query failed: {e}')
        return []


def download_icesat2_granule(
    granule: dict,
    output_dir: Path,
    token: str,
) -> Path | None:
    """Download a single ICESat-2 granule."""
    if not granule.get('dl_url'):
        return None
    
    filename = granule['granule_id'].replace('/', '_') + '.h5'
    output_path = output_dir / filename
    
    if output_path.exists() and output_path.stat().st_size > 0:
        print(f'  ✓ Already downloaded: {filename}')
        return output_path
    
    print(f'  Downloading {filename}...')
    
    headers = {
        'Authorization': f'Bearer {token}',
        'Accept': 'application/octet-stream',
    }
    
    try:
        resp = requests.get(granule['dl_url'], headers=headers, timeout=600, stream=True)
        resp.raise_for_status()
        
        with open(output_path, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=8192):
                if chunk:
                    f.write(chunk)
        
        size_mb = output_path.stat().st_size / 1e6
        print(f'  ✓ Saved: {filename} ({size_mb:.1f} MB)')
        return output_path
        
    except Exception as e:
        print(f'  ✗ Download failed: {e}')
        if output_path.exists():
            output_path.unlink()
        return None


def batch_download_icesat2(
    bbox: dict = None,
    max_granules: int = 50,
) -> dict:
    """
    Batch download ICESat-2 ATL13 granules.
    
    Searches ALL available dates (2018-present) since coverage is so sparse.
    """
    print('='*70)
    print('ICESat-2 ATL13 BATCH DOWNLOAD')
    print('='*70)
    print()
    
    if bbox is None:
        bbox = LAKE_MICHIGAN_BBOX
    
    token = load_earthdata_token()
    if not token:
        print('ERROR: No Earthdata token found!')
        return {'downloaded': 0}
    
    print(f'✓ Earthdata token loaded')
    print(f'Bounding Box: {bbox}')
    print(f'Max Granules: {max_granules}')
    print()
    
    # ICESat-2 launched Oct 2018
    print('NOTE: ICESat-2 launched October 14, 2018')
    print('      For 2012 low water data, use Landsat 5/7 instead')
    print()
    
    # Query by year - PRIORITIZE LOW WATER PERIODS!
    all_granules = []
    
    # 2025 LOW WATER (BEST!): January 2025 - present
    print('[1/4] 2025 LOW WATER (BEST - lowest in 10 years!)...')
    low_2025 = query_icesat2_granules(bbox, ('2025-01-01', '2025-12-31'), token, max_results=20)
    all_granules.extend(low_2025)
    print()
    
    # 2024 MODERATE LOW: Full year
    print('[2/4] 2024 (moderate low water)...')
    year_2024 = query_icesat2_granules(bbox, ('2024-01-01', '2024-12-31'), token, max_results=20)
    all_granules.extend(year_2024)
    print()
    
    # 2022-2023 DECLINE: When water started dropping
    print('[3/4] 2022-2023 (water level decline)...')
    decline = query_icesat2_granules(bbox, ('2022-01-01', '2023-12-31'), token, max_results=20)
    all_granules.extend(decline)
    print()
    
    # 2019-2021 HIGH WATER (for comparison/baseline)
    print('[4/4] 2019-2021 (high water baseline for comparison)...')
    high_water = query_icesat2_granules(bbox, ('2019-01-01', '2021-12-31'), token, max_results=20)
    all_granules.extend(high_water)
    print()
    
    print()
    print(f'Total granules available: {len(all_granules)}')
    print()
    
    if not all_granules:
        print('No ICESat-2 granules found for Lake Michigan.')
        print('This is normal - ICESat-2 has very sparse coverage.')
        print('Consider expanding the bounding box or checking NSIDC directly.')
        return {'downloaded': 0}
    
    # Show sample
    print('Available granules:')
    for i, g in enumerate(all_granules[:5], 1):
        print(f'  {i}. {g["time_start"][:10]} - {g["title"][:60]}...')
    if len(all_granules) > 5:
        print(f'  ... and {len(all_granules) - 5} more')
    print()
    
    # Download
    print(f'Downloading up to {min(len(all_granules), max_granules)} granules...')
    print(f'Estimated size: ~{min(len(all_granules), max_granules) * 100:.0f} MB (100 MB avg)')
    print()
    
    downloaded = []
    total_size = 0
    
    for i, granule in enumerate(all_granules[:max_granules], 1):
        print(f'[{i}/{min(len(all_granules), max_granules)}]')
        result = download_icesat2_granule(granule, OUTPUT_DIR, token)
        if result:
            downloaded.append({
                'granule_id': granule['granule_id'],
                'file_path': str(result),
                'size_mb': result.stat().st_size / 1e6,
                'time_start': granule['time_start'],
            })
            total_size += result.stat().st_size
        print()
        
        time.sleep(0.5)
    
    # Save summary
    summary = {
        'downloaded_at': datetime.now().isoformat(),
        'bbox': bbox,
        'granules_available': len(all_granules),
        'granules_downloaded': len(downloaded),
        'total_size_mb': total_size / 1e6,
        'total_size_gb': total_size / 1e9,
        'downloads': downloaded,
    }
    
    summary_path = OUTPUT_DIR / 'icesat2_atl13_summary.json'
    with open(summary_path, 'w') as f:
        json.dump(summary, f, indent=2)
    
    print('='*70)
    print('ICESat-2 BATCH DOWNLOAD COMPLETE')
    print('='*70)
    print(f'Available: {len(all_granules)} granules')
    print(f'Downloaded: {len(downloaded)} granules')
    print(f'Total size: {total_size/1e9:.2f} GB')
    print(f'Output: {OUTPUT_DIR}')
    print('='*70)
    
    return summary


if __name__ == '__main__':
    # Download all available ICESat-2 ATL13 granules
    # Note: Very sparse coverage, may only get a few granules
    batch_download_icesat2(max_granules=50)
