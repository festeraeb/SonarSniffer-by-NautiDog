"""
swot_batch_downloader.py

Batch downloads ALL available SWOT L2_LR_SSH Expert granules for Lake Michigan.

Uses your full 100GB bandwidth - downloads everything with good coverage.

Focus areas:
  - Lake Michigan corridor (Zion/Waukegan to Milwaukee)
  - Date ranges: 
    * Historical: 2023-01-01 to 2025-07-31 (before Rossa)
    * Recent: 2025-08-01 to present (after Rossa sinking)
"""

import json
import os
import time
from datetime import datetime, timedelta
from pathlib import Path
import requests

# ── Configuration ─────────────────────────────────────────────────────────────

# Lake Michigan bounding box (corridor + full lake)
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9,
    'lat_min': 41.5,
    'lon_max': -85.5,
    'lat_max': 46.0,
}

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'swot_ssh'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Earthdata token
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
]

# NASA CMR API
CMR_BASE = 'https://cmr.earthdata.nasa.gov/search/granules.json'
PODAAC_BASE = 'https://archive.swot.podaac.earthdata.nasa.gov/podaac-swot-ops-cumulus-protected'

# SWOT product
SWOT_PRODUCT = 'SWOT_L2_LR_SSH_2.0'  # Expert level

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


def query_swot_granules(
    bbox: dict,
    date_range: tuple[str, str],
    token: str,
    max_results: int = 500,
) -> list[dict]:
    """
    Query NASA CMR for SWOT SSH granules.
    
    Returns list of granule metadata with download URLs.
    """
    headers = {
        'Accept': 'application/json',
        'Authorization': f'Bearer {token}',
    }
    
    params = {
        'short_name': SWOT_PRODUCT,
        'temporal': f'{date_range[0]}T00:00:00Z,{date_range[1]}T23:59:59Z',
        'bounding_box': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'page_size': min(max_results, 2000),
        'sort_key': 'start_date',
    }
    
    print(f'Querying CMR for SWOT granules ({date_range[0]} to {date_range[1]})...')
    
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
                    'file_size': entry.get('data_center', ''),
                })
        
        print(f'  Found {len(granules)} granules with download URLs')
        return granules
        
    except Exception as e:
        print(f'  CMR query failed: {e}')
        return []


def download_swot_granule(
    granule: dict,
    output_dir: Path,
    token: str,
) -> Path | None:
    """
    Download a single SWOT granule.
    
    Returns path to downloaded file, or None if failed.
    """
    if not granule.get('dl_url'):
        return None
    
    # Create filename from granule ID
    filename = granule['granule_id'].replace('/', '_') + '.nc'
    output_path = output_dir / filename
    
    # Skip if already downloaded
    if output_path.exists() and output_path.stat().st_size > 0:
        print(f'  ✓ Already downloaded: {filename}')
        return output_path
    
    # Download
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


def batch_download_swot(
    bbox: dict = None,
    max_granules: int = 100,  # Download up to 100 granules (use your 100GB!)
) -> dict:
    """
    Batch download ALL available SWOT SSH granules.
    
    Args:
        bbox: Bounding box (default: Lake Michigan)
        max_granules: Maximum granules to download (default: 100)
    
    Returns:
        Summary dict
    """
    print('='*70)
    print('SWOT SSH BATCH DOWNLOAD')
    print('='*70)
    print()
    
    if bbox is None:
        bbox = LAKE_MICHIGAN_BBOX
    
    # Load token
    token = load_earthdata_token()
    if not token:
        print('ERROR: No Earthdata token found!')
        return {'downloaded': 0}
    
    print(f'✓ Earthdata token loaded')
    print(f'Bounding Box: {bbox}')
    print(f'Max Granules: {max_granules}')
    print()
    
    # Query SWOT for ALL AVAILABLE DATA (2023-present)
    # We're looking for PERMANENT surface disturbances from:
    # 1. Flight 2501 debris field (DC-4, 1950, 75 years on bottom)
    # 2. Andaste wreck (Whaleback, 1907, 119 years on bottom)
    # These have been there LONG before SWOT launched - we're detecting the surface signature
    all_granules = []
    
    # Get current date
    from datetime import datetime
    end_date = datetime.now().strftime('%Y-%m-%d')

    # FULL SWOT COVERAGE (2023-present)
    print('[1/1] FULL SWOT COVERAGE (2023-present) - PERMANENT SIGNATURES...')
    print('      Searching for surface disturbances from:')
    print('        - Flight 2501 debris field (93.5 mi from South Haven)')
    print('        - Andaste wreck (Zion/Waukegan trench)')
    all_granules = query_swot_granules(bbox, ('2023-01-01', end_date), token, max_results=max_granules)
    print()
    
    print(f'Total granules available: {len(all_granules)}')
    print()
    
    if not all_granules:
        print('No SWOT granules found for this area/date range.')
        return {'downloaded': 0}
    
    # Show sample
    print('Sample granules:')
    for i, g in enumerate(all_granules[:5], 1):
        print(f'  {i}. {g["title"][:70]}...')
        print(f'     {g["time_start"][:10]} to {g["time_end"][:10]}')
    if len(all_granules) > 5:
        print(f'  ... and {len(all_granules) - 5} more')
    print()
    
    # Download
    print(f'Downloading up to {min(len(all_granules), max_granules)} granules...')
    print(f'Estimated size: ~{min(len(all_granules), max_granules) * 50:.0f} MB (50 MB avg per granule)')
    print()
    
    downloaded = []
    total_size = 0
    
    for i, granule in enumerate(all_granules[:max_granules], 1):
        print(f'[{i}/{min(len(all_granules), max_granules)}]')
        result = download_swot_granule(granule, OUTPUT_DIR, token)
        if result:
            downloaded.append({
                'granule_id': granule['granule_id'],
                'file_path': str(result),
                'size_mb': result.stat().st_size / 1e6,
                'time_start': granule['time_start'],
            })
            total_size += result.stat().st_size
        print()
        
        # Be nice to NASA servers
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
    
    summary_path = OUTPUT_DIR / 'swot_ssh_summary.json'
    with open(summary_path, 'w') as f:
        json.dump(summary, f, indent=2)
    
    print('='*70)
    print('SWOT BATCH DOWNLOAD COMPLETE')
    print('='*70)
    print(f'Available: {len(all_granules)} granules')
    print(f'Downloaded: {len(downloaded)} granules')
    print(f'Total size: {total_size/1e9:.2f} GB')
    print(f'Output: {OUTPUT_DIR}')
    print(f'Summary: {summary_path}')
    print('='*70)
    
    return summary


if __name__ == '__main__':
    # Download ALL available SWOT SSH granules (up to 100)
    # SWOT launched Dec 2022, so we only get 2023-present data
    # For 2012 low water, we need Landsat 5/7 instead
    print('NOTE: SWOT only available 2023-present (launched Dec 2022)')
    print('For 2012 low water data, use landsat_thermal_fetcher.py instead')
    print()
    batch_download_swot(max_granules=100)
