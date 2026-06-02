"""
landsat_thermal_fetcher.py

Downloads Landsat 8/9 TIRS (Thermal Infrared Sensor) data for Lake Michigan.

Uses ASF (Alaska Satellite Facility) Vertex API for better coverage.

Bands:
  - B10: Thermal Infrared (10.6-11.2 μm, 100m resolution)
  - B11: Thermal Infrared (11.5-12.5 μm, 100m resolution)

Use case: Detect thermal sinks (cold spots) from lead keels, wrecks, etc.
"""

import json
import os
import time
from datetime import datetime, timedelta
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
OUTPUT_DIR = REPO / 'outputs' / 'landsat_thermal'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Earthdata token paths
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
]

# ASF Vertex API (better Landsat coverage)
ASF_SEARCH = 'https://vertex.daac.asf.alaska.edu/search'
ASF_DOWNLOAD = 'https://datapool.asf.alaska.edu/'

# USGS EarthExplorer fallback
USGS_API = 'https://earthexplorer.usgs.gov/inventory/json/v/1.4.0'

# Landsat products
LANDSAT_PRODUCTS = [
    'LANDSAT_OT_C2_L2',  # Landsat 8 OLI/TIRS Collection 2 Level-2
    'LANDSAT_TM_C2_L2',  # Landsat 7 ETM+ Collection 2 Level-2 (fallback)
]

# ── Helpers ───────────────────────────────────────────────────────────────────

def load_earthdata_token() -> str:
    """Load Earthdata token from file."""
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    data = json.loads(tp.read_text(encoding='utf-8'))
                    return data.get('earthdata_token', '')
                else:
                    return tp.read_text(encoding='utf-8').strip()
            except Exception:
                continue
    return ''


def bbox_to_string(bbox: dict) -> str:
    """Convert bbox dict to CMR format: lon_min,lat_min,lon_max,lat_max"""
    return f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}"


# ── Landsat Download ──────────────────────────────────────────────────────────

def query_landsat_thermal(
    bbox: dict,
    start_date: str,
    end_date: str,
    token: str = '',
    max_results: int = 100,
) -> list[dict]:
    """
    Query ASF Vertex for Landsat thermal granules.
    
    Searches ALL historical data during ice-free periods (May-Nov).
    
    Args:
        bbox: Bounding box dict
        start_date: Start date (YYYY-MM-DD)
        end_date: End date (YYYY-MM-DD)
        token: Earthdata token
        max_results: Maximum granules to return
    
    Returns:
        List of granule metadata dicts
    """
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    
    all_granules = []
    
    # Search by ice-free months only (May-November)
    # Split into multiple queries by year/month for better coverage
    print('  Searching historical ice-free periods (May-Nov)...')
    
    for product in LANDSAT_PRODUCTS:
        params = {
            'dataset': product.replace('_', '-'),  # ASF uses dashes
            'boundingBox': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
            'startDate': start_date,
            'endDate': end_date,
            'maxResults': max_results,
            'sort': 'startTime',
        }
        
        try:
            resp = requests.get(ASF_SEARCH, params=params, headers=headers, timeout=120)
            resp.raise_for_status()
            entries = resp.json()
            
            if isinstance(entries, list) and entries:
                print(f'  {product}: {len(entries)} granules found')
                
                for entry in entries:
                    # Extract metadata
                    granule = {
                        'product': product,
                        'granule_id': entry.get('granuleName', entry.get('title', '')),
                        'title': entry.get('title', ''),
                        'time_start': entry.get('startTime', ''),
                        'time_end': entry.get('stopTime', ''),
                        'cloud_cover': entry.get('cloudCover', entry.get('cloud_cover', 'N/A')),
                        'dl_url': entry.get('downloadUrl', entry.get('url', '')),
                    }
                    all_granules.append(granule)
                
                # Got results, stop trying fallbacks
                break
            else:
                print(f'  {product}: 0 granules')
                
        except Exception as e:
            print(f'  {product} ASF query failed: {e}')
    
    return all_granules


def query_usgs_earthexplorer(
    bbox: dict,
    start_date: str,
    end_date: str,
    max_results: int = 50,
) -> list[dict]:
    """
    Query USGS EarthExplorer for Landsat thermal granules.
    
    Fallback when ASF doesn't have coverage.
    
    Args:
        bbox: Bounding box dict
        start_date: Start date (YYYY-MM-DD)
        end_date: End date (YYYY-MM-DD)
        max_results: Maximum granules to return
    
    Returns:
        List of granule metadata dicts
    """
    print('  Trying USGS EarthExplorer fallback...')
    
    # USGS EarthExplorer doesn't have a simple public API
    # We'll use their inventory JSON endpoint
    all_granules = []
    
    for product in ['landsat_ot_c2_l2', 'landsat_tm_c2_l2']:
        try:
            # Build query for ice-free period
            url = f'{USGS_API}/datasets/{product}/results'
            params = {
                'bbox': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
                'start_date': start_date,
                'end_date': end_date,
                'max_results': max_results,
            }
            
            resp = requests.get(url, params=params, timeout=60)
            
            if resp.status_code == 200:
                data = resp.json()
                if isinstance(data, dict) and 'results' in data:
                    entries = data['results']
                    print(f'  {product.upper()}: {len(entries)} granules found')
                    
                    for entry in entries:
                        granule = {
                            'product': product.upper(),
                            'granule_id': entry.get('displayId', entry.get('entityId', '')),
                            'title': entry.get('displayId', ''),
                            'time_start': entry.get('temporal', {}).get('startDate', ''),
                            'time_end': entry.get('temporal', {}).get('endDate', ''),
                            'cloud_cover': entry.get('cloudCover', 'N/A'),
                            'dl_url': entry.get('downloadUrl', ''),
                        }
                        all_granules.append(granule)
                    
                    break
                else:
                    print(f'  {product.upper()}: 0 granules')
            else:
                print(f'  {product.upper()}: HTTP {resp.status_code}')
                
        except Exception as e:
            print(f'  {product.upper()} USGS query failed: {e}')
    
    return all_granules


def download_landsat_granule(
    granule: dict,
    output_dir: Path,
    token: str,
) -> Path | None:
    """
    Download a single Landsat granule.
    
    Args:
        granule: Granule metadata dict
        output_dir: Output directory
        token: Earthdata token
    
    Returns:
        Path to downloaded file, or None if failed
    """
    if not granule.get('dl_url'):
        print(f'  No download URL for {granule["granule_id"]}')
        return None
    
    # Create filename from granule ID
    filename = granule['granule_id'].replace('/', '_') + '.zip'
    output_path = output_dir / filename
    
    # Skip if already downloaded
    if output_path.exists() and output_path.stat().st_size > 0:
        print(f'  Already downloaded: {filename}')
        return output_path
    
    # Download
    print(f'  Downloading {filename}...')
    
    headers = {
        'Authorization': f'Bearer {token}',
        'Accept': 'application/octet-stream',
    }
    
    try:
        resp = requests.get(granule['dl_url'], headers=headers, timeout=300, stream=True)
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


def fetch_landsat_thermal(
    bbox: dict = None,
    start_year: int = 2013,
    end_year: int = None,
    start_month: int = None,
    end_month: int = None,
    start_day: int = None,
    end_day: int = None,
    max_granules: int = 50,
) -> dict:
    """
    Main function: Fetch ALL historical Landsat thermal data for Lake Michigan.
    
    Searches ice-free periods (May-November) across ALL available years.
    
    Args:
        bbox: Bounding box (default: Lake Michigan)
        start_year: First year to search (default: 2013, Landsat 8 launch)
        end_year: Last year to search (default: current year)
        max_granules: Maximum granules to download
    
    Returns:
        Summary dict with results
    """
    print('='*70)
    print('LANDSAT THERMAL - HISTORICAL ICE-FREE SEARCH')
    print('='*70)
    print()
    
    # Defaults
    if bbox is None:
        bbox = LAKE_MICHIGAN_BBOX
    if end_year is None:
        end_year = datetime.now().year
    
    # Search ice-free periods: May 1 - November 30 for each year
    print(f'Bounding Box: {bbox}')
    print(f'Year Range: {start_year} to {end_year} ({end_year - start_year + 1} years)')
    print(f'Search Window: May 1 - November 30 (ice-free period)')
    print(f'Max Granules: {max_granules}')
    print()
    
    # Load token
    token = load_earthdata_token()
    if token:
        print(f'✓ Earthdata token loaded')
    else:
        print('⚠ No Earthdata token — downloads may fail')
    print()
    
    # Query for each year's ice-free period
    all_granules = []
    
    for year in range(start_year, end_year + 1):
        print(f'[{year}] Searching...')
        
        # Use provided month/day or default to ice-free period
        if start_month and end_month:
            start_date = f'{year}-{start_month:02d}-{start_day or 1:02d}'
            end_date = f'{year}-{end_month:02d}-{end_day or 28:02d}'
        else:
            # Default: ice-free period (May-Nov)
            start_date = f'{year}-05-01'
            end_date = f'{year}-11-30'
        
        # Try ASF first
        granules = query_landsat_thermal(bbox, start_date, end_date, token, max_results=20)
        
        # Fallback to USGS if ASF found nothing
        if not granules:
            granules = query_usgs_earthexplorer(bbox, start_date, end_date, max_results=20)
        
        all_granules.extend(granules)
        
        # Limit total
        if len(all_granules) >= max_granules:
            all_granules = all_granules[:max_granules]
            break
        
        # Be nice to servers
        time.sleep(0.5)
    
    print()
    print(f'Total granules found: {len(all_granules)}')
    print()
    
    if not all_granules:
        print('No Landsat thermal data found.')
        print('Try expanding the year range or checking cloud cover filters.')
        return {'granules_found': 0, 'downloaded': 0}
    
    # Show granule info
    print('Granules available:')
    for i, g in enumerate(granules[:10], 1):
        cloud = g.get('cloud_cover', 'N/A')
        print(f'  {i}. {g["title"][:60]}... (cloud: {cloud}%)')
    if len(granules) > 10:
        print(f'  ... and {len(granules) - 10} more')
    print()
    
    # Download
    print(f'Downloading up to {max_granules} granules...')
    print()
    
    downloaded = []
    for i, granule in enumerate(granules[:max_granules], 1):
        print(f'[{i}/{len(granules[:max_granules])}]')
        result = download_landsat_granule(granule, OUTPUT_DIR, token)
        if result:
            downloaded.append({
                'granule_id': granule['granule_id'],
                'file_path': str(result),
                'size_mb': result.stat().st_size / 1e6,
            })
        print()
        
        # Be nice to NASA servers
        time.sleep(1)
    
    # Save summary
    summary = {
        'fetched_at': datetime.now().isoformat(),
        'bbox': bbox,
        'date_range': date_range,
        'granules_found': len(granules),
        'granules_downloaded': len(downloaded),
        'downloads': downloaded,
    }
    
    summary_path = OUTPUT_DIR / 'landsat_thermal_summary.json'
    with open(summary_path, 'w') as f:
        json.dump(summary, f, indent=2)
    
    print('='*70)
    print('LANDSAT THERMAL FETCH COMPLETE')
    print('='*70)
    print(f'Granules found: {len(granules)}')
    print(f'Downloaded: {len(downloaded)}')
    print(f'Output: {OUTPUT_DIR}')
    print(f'Summary: {summary_path}')
    print('='*70)
    
    return summary


if __name__ == '__main__':
    # Fetch Landsat thermal for Rossa investigation
    # CRITICAL: Before/After Aug 22, 2025 (sinking date)
    
    print('='*70)
    print('LANDSAT THERMAL - ROSSA INVESTIGATION')
    print('='*70)
    print()
    print('Rossa Timeline:')
    print('  Last seen: Aug 22, 2025 ~11 AM (McKinley Marina)')
    print('  Last position: Aug 22, 2025 ~3 PM (8 miles out)')
    print('  Sank: Aug 22, 2025 before midnight')
    print()
    print('Searching for thermal data:')
    print('  BEFORE: Jan 1 - Aug 21, 2025 (NO Rossa in water)')
    print('  AFTER: Aug 23 - present (ROSSA IN WATER)')
    print()
    
    from landsat_thermal_fetcher import fetch_landsat_thermal
    
    # BEFORE Rossa (baseline - no wreck)
    print('[BEFORE ROSSA - Baseline]')
    fetch_landsat_thermal(
        start_year=2025,
        end_year=2025,
        start_month=1,
        end_month=8,
        end_day=21,  # Before sinking
        max_granules=20,
    )
    
    print()
    print('[AFTER ROSSA - Wreck in water]')
    fetch_landsat_thermal(
        start_year=2025,
        end_year=2025,
        start_month=8,
        end_month=12,
        start_day=23,  # After sinking
        max_granules=20,
    )
