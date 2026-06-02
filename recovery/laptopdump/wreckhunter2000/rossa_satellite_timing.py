"""
rossa_satellite_timing.py

Finds satellite passes over the Rossa search zone during/after sinking.

Sinking Time: August 22, 2025, 8-9 PM CDT (00:00-01:00 UTC Aug 23)
Location: 8 miles out from McKinley + 15 NM arc

This queries for:
1. Sentinel-2 (10m optical) - overpass times
2. Landsat 8/9 (thermal) - overpass times
3. SWOT (height) - overpass times
4. ICESat-2 (laser) - overpass times
5. Sentinel-1 (SAR) - overpass times

Priority: Passes within 24 hours of sinking (Aug 22-23, 2025)
"""

import json
from datetime import datetime, timedelta
from pathlib import Path
import requests

# ── Configuration ─────────────────────────────────────────────────────────────

# Rossa sinking parameters
SINKING_TIME = {
    'date': '2025-08-22',
    'time_cdt': '20:00-21:00',  # 8-9 PM CDT
    'time_utc': '2025-08-23T00:00-01:00',  # 00:00-01:00 UTC Aug 23
}

# Search zone (8 miles out + 15 NM arc)
SEARCH_ZONE = {
    'center_lat': 43.0167,
    'center_lon': -87.734,
    'radius_nm': 15,
}

# Time windows for satellite search
TIME_WINDOWS = [
    {'name': 'DURING SINKING', 'start': '2025-08-22T19:00', 'end': '2025-08-23T02:00'},  # 8-9 PM +/- buffer
    {'name': 'IMMEDIATELY AFTER', 'start': '2025-08-23T02:00', 'end': '2025-08-23T12:00'},  # Next morning
    {'name': '1-3 DAYS AFTER', 'start': '2025-08-23T12:00', 'end': '2025-08-25T23:59'},  # Critical window
    {'name': '1 WEEK AFTER', 'start': '2025-08-26T00:00', 'end': '2025-08-29T23:59'},  # Debris still fresh
]

# NASA CMR API
CMR_BASE = 'https://cmr.earthdata.nasa.gov/search/granules.json'

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'rossa_satellite_timing'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ── Satellite Queries ─────────────────────────────────────────────────────────

def query_satellite_passes(
    short_name: str,
    time_window: dict,
    bbox: dict,
    token: str = '',
) -> list[dict]:
    """
    Query NASA CMR for satellite passes over search zone.
    
    Returns list of granules with overpass times.
    """
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    
    params = {
        'short_name': short_name,
        'temporal': f"{time_window['start']}Z,{time_window['end']}Z",
        'bounding_box': f"{bbox['center_lon']-0.5},{bbox['center_lat']-0.5},{bbox['center_lon']+0.5},{bbox['center_lat']+0.5}",
        'page_size': 50,
        'sort_key': 'start_date',
    }
    
    try:
        resp = requests.get(CMR_BASE, params=params, headers=headers, timeout=60)
        resp.raise_for_status()
        
        entries = resp.json().get('feed', {}).get('entry', [])
        
        passes = []
        for entry in entries:
            passes.append({
                'granule_id': entry.get('id', ''),
                'time_start': entry.get('time_start', ''),
                'time_end': entry.get('time_end', ''),
                'title': entry.get('title', ''),
            })
        
        return passes
        
    except Exception as e:
        print(f'  Query failed: {e}')
        return []


def find_optimal_passes() -> dict:
    """
    Find optimal satellite passes for Rossa search.
    
    Returns dict with passes by satellite type and time window.
    """
    # Load Earthdata token
    token = ''
    token_paths = [
        Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
    ]
    for tp in token_paths:
        if tp.exists():
            token = json.loads(tp.read_text(encoding='utf-8')).get('earthdata_token', '')
            break
    
    print('='*70)
    print('ROSSA SATELLITE TIMING ANALYSIS')
    print('='*70)
    print()
    print(f'Sinking Time: {SINKING_TIME["date"]} {SINKING_TIME["time_cdt"]} CDT')
    print(f'              {SINKING_TIME["time_utc"]} UTC')
    print(f'Search Zone: {SEARCH_ZONE["radius_nm"]} NM arc, 8 miles from McKinley')
    print(f'Center: {SEARCH_ZONE["center_lat"]:.4f}°N, {SEARCH_ZONE["center_lon"]:.4f}°W')
    print()
    
    # Query each time window
    results = {
        'sinking_time': [],
        'immediately_after': [],
        '1_3_days_after': [],
        '1_week_after': [],
    }
    
    # Sentinel-2 (BEST for optical wreck detection)
    print('Searching Sentinel-2 L2A (10m optical)...')
    for window in TIME_WINDOWS:
        passes = query_satellite_passes('SENTINEL-2_L2A', window, SEARCH_ZONE, token)
        if passes:
            print(f'  {window["name"]}: {len(passes)} passes')
            for p in passes[:3]:  # Show first 3
                print(f'    {p["time_start"][:19]} - {p["title"][:50]}...')
            
            if window['name'] == 'DURING SINKING':
                results['sinking_time'].extend(passes)
            elif window['name'] == 'IMMEDIATELY AFTER':
                results['immediately_after'].extend(passes)
            elif window['name'] == '1-3 DAYS AFTER':
                results['1_3_days_after'].extend(passes)
            elif window['name'] == '1 WEEK AFTER':
                results['1_week_after'].extend(passes)
    print()
    
    # Landsat 8/9 (thermal)
    print('Searching Landsat 8/9 TIRS (thermal)...')
    for window in TIME_WINDOWS:
        passes = query_satellite_passes('LANDSAT_OT_C2_L2', window, SEARCH_ZONE, token)
        if passes:
            print(f'  {window["name"]}: {len(passes)} passes')
    print()
    
    # SWOT (height anomalies)
    print('Searching SWOT SSH (height anomalies)...')
    for window in TIME_WINDOWS:
        passes = query_satellite_passes('SWOT_L2_LR_SSH_2.0', window, SEARCH_ZONE, token)
        if passes:
            print(f'  {window["name"]}: {len(passes)} passes')
    print()
    
    # ICESat-2 (laser altimetry)
    print('Searching ICESat-2 ATL13 (laser)...')
    for window in TIME_WINDOWS:
        passes = query_satellite_passes('ATL13', window, SEARCH_ZONE, token)
        if passes:
            print(f'  {window["name"]}: {len(passes)} passes')
    print()
    
    # Save results
    summary_path = OUTPUT_DIR / 'rossa_satellite_timing_summary.json'
    with open(summary_path, 'w') as f:
        json.dump({
            'generated_at': datetime.now().isoformat(),
            'sinking_time': SINKING_TIME,
            'search_zone': SEARCH_ZONE,
            'time_windows': TIME_WINDOWS,
            'results': {k: len(v) for k, v in results.items()},
        }, f, indent=2)
    
    print('='*70)
    print(f'Summary saved: {summary_path}')
    print('='*70)
    
    return results


if __name__ == '__main__':
    find_optimal_passes()
