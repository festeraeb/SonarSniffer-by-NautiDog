"""
recent_satellite_coverage.py

Search for ALL satellite coverage post-August 2025 over:
1. Andaste cluster (42.47°N, -87.10°W)
2. Milwaukee corridor (42.49-43.05°N, -87.95°W)

Sensors:
- Sentinel-2 (Optical, 10m)
- Sentinel-1 (SAR, 5m)
- Landsat 8/9 (Thermal, 100m)
- ICESat-2 (Laser, 17m footprint)
- SWOT (Altimetry, 1cm height)

Goal: Find most recent data available NOW
"""

import json
import requests
from datetime import datetime, timedelta
from pathlib import Path

# ── Target Areas ──────────────────────────────────────────────────────────────

TARGETS = {
    'ANDASTE': {
        'name': 'Andaste Cluster',
        'bbox': [-87.12, 42.44, -87.06, 42.49],  # lon_min, lat_min, lon_max, lat_max
    },
    'MILWAUKEE': {
        'name': 'Milwaukee Corridor',
        'bbox': [-87.95, 42.49, -87.80, 43.05],
    },
}

# Date threshold: post-August 2025
DATE_FROM = '2025-08-25T00:00:00Z'
DATE_TO = datetime.now().strftime('%Y-%m-%dT23:59:59Z')

# ── API Endpoints ─────────────────────────────────────────────────────────────

# Sentinel-1/2 STAC (Earth-search)
STAC_API = 'https://earth-search.aws.element84.com/v1/search'

# Landsat CMR
CMR_API = 'https://cmr.earthdata.nasa.gov/search/granules.json'

# ICESat-2 CMR
ICESAT2_CMR = 'https://cmr.earthdata.nasa.gov/search/granules.json'

# ── Search Functions ──────────────────────────────────────────────────────────

def search_sentinel2_stac(bbox: list, date_from: str, date_to: str) -> dict:
    """Search Sentinel-2 L2A via STAC"""
    payload = {
        'collections': ['sentinel-2-l2a'],
        'bbox': bbox,
        'datetime': f'{date_from}/{date_to}',
        'limit': 100,
    }
    
    try:
        resp = requests.post(STAC_API, json=payload, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        features = data.get('features', [])
        
        return {
            'sensor': 'Sentinel-2 L2A',
            'count': len(features),
            'scenes': [
                {
                    'id': f.get('id'),
                    'datetime': f.get('properties', {}).get('datetime'),
                    'cloud_cover': f.get('properties', {}).get('eo:cloud_cover'),
                    'mgrs_tile': f.get('properties', {}).get('mgrs:utm_zone'),
                }
                for f in features[:20]
            ]
        }
    except Exception as e:
        return {'sensor': 'Sentinel-2 L2A', 'error': str(e), 'count': 0}


def search_sentinel1_stac(bbox: list, date_from: str, date_to: str) -> dict:
    """Search Sentinel-1 GRD via STAC"""
    payload = {
        'collections': ['sentinel-1-grd'],
        'bbox': bbox,
        'datetime': f'{date_from}/{date_to}',
        'query': {
            'sar:product_type': {'eq': 'GRD'},
            'sar:polarizations': {'contains': 'VV'},
        },
        'limit': 100,
    }
    
    try:
        resp = requests.post(STAC_API, json=payload, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        features = data.get('features', [])
        
        return {
            'sensor': 'Sentinel-1 GRD',
            'count': len(features),
            'scenes': [
                {
                    'id': f.get('id'),
                    'datetime': f.get('properties', {}).get('datetime'),
                    'orbit': f.get('properties', {}).get('sat:relative_orbit'),
                    'polarizations': f.get('properties', {}).get('sar:polarizations'),
                }
                for f in features[:20]
            ]
        }
    except Exception as e:
        return {'sensor': 'Sentinel-1 GRD', 'error': str(e), 'count': 0}


def search_landsat_cmr(bbox: list, date_from: str, date_to: str) -> dict:
    """Search Landsat 8/9 via CMR"""
    params = {
        'short_name': 'LANDSAT_OT_C2_L2',
        'temporal': f'{date_from},{date_to}',
        'bounding_box': f'{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}',
        'page_size': 100,
    }
    
    try:
        resp = requests.get(CMR_API, params=params, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        entries = data.get('feed', {}).get('entry', [])
        
        return {
            'sensor': 'Landsat 8/9 OLI/TIRS',
            'count': len(entries),
            'scenes': [
                {
                    'id': e.get('id'),
                    'datetime': e.get('time_start'),
                    'cloud_cover': e.get('cloud_cover'),
                }
                for e in entries[:20]
            ]
        }
    except Exception as e:
        return {'sensor': 'Landsat 8/9', 'error': str(e), 'count': 0}


def search_icesat2_cmr(bbox: list, date_from: str, date_to: str) -> dict:
    """Search ICESat-2 ATL13 via CMR"""
    params = {
        'short_name': 'ATL13',
        'temporal': f'{date_from},{date_to}',
        'bounding_box': f'{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}',
        'page_size': 100,
    }
    
    try:
        resp = requests.get(ICESAT2_CMR, params=params, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        entries = data.get('feed', {}).get('entry', [])
        
        return {
            'sensor': 'ICESat-2 ATL13',
            'count': len(entries),
            'scenes': [
                {
                    'id': e.get('id'),
                    'datetime': e.get('time_start'),
                }
                for e in entries[:20]
            ]
        }
    except Exception as e:
        return {'sensor': 'ICESat-2 ATL13', 'error': str(e), 'count': 0}


def search_swot_cmr(bbox: list, date_from: str, date_to: str) -> dict:
    """Search SWOT SSH via CMR"""
    params = {
        'short_name': 'SWOT_L2_LR_SSH_2.0',
        'temporal': f'{date_from},{date_to}',
        'bounding_box': f'{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}',
        'page_size': 100,
    }
    
    try:
        resp = requests.get(CMR_API, params=params, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        entries = data.get('feed', {}).get('entry', [])
        
        return {
            'sensor': 'SWOT SSH',
            'count': len(entries),
            'scenes': [
                {
                    'id': e.get('id'),
                    'datetime': e.get('time_start'),
                    'product': e.get('title'),
                }
                for e in entries[:20]
            ]
        }
    except Exception as e:
        return {'sensor': 'SWOT SSH', 'error': str(e), 'count': 0}


# ── Main Search ───────────────────────────────────────────────────────────────

def main():
    print('='*80)
    print('RECENT SATELLITE COVERAGE SEARCH')
    print('Post-August 2025 Data Availability')
    print('='*80)
    print()
    print(f'Date Range: {DATE_FROM} to {DATE_TO}')
    print()
    
    all_results = {}
    
    for target_name, target_data in TARGETS.items():
        print('='*80)
        print(f'TARGET: {target_data["name"]}')
        print(f'Bbox: {target_data["bbox"]}')
        print('='*80)
        print()
        
        results = {
            'target': target_name,
            'bbox': target_data['bbox'],
            'sensors': {}
        }
        
        # Search each sensor
        print('Searching sensors...')
        print()
        
        # Sentinel-2
        print('  Sentinel-2 L2A (Optical)...')
        s2_result = search_sentinel2_stac(target_data['bbox'], DATE_FROM, DATE_TO)
        results['sensors']['Sentinel-2'] = s2_result
        print(f'    Found: {s2_result.get("count", 0)} scenes')
        
        # Sentinel-1
        print('  Sentinel-1 GRD (SAR)...')
        s1_result = search_sentinel1_stac(target_data['bbox'], DATE_FROM, DATE_TO)
        results['sensors']['Sentinel-1'] = s1_result
        print(f'    Found: {s1_result.get("count", 0)} scenes')
        
        # Landsat
        print('  Landsat 8/9 (Thermal)...')
        ls_result = search_landsat_cmr(target_data['bbox'], DATE_FROM, DATE_TO)
        results['sensors']['Landsat'] = ls_result
        print(f'    Found: {ls_result.get("count", 0)} scenes')
        
        # ICESat-2
        print('  ICESat-2 ATL13 (Laser)...')
        ic_result = search_icesat2_cmr(target_data['bbox'], DATE_FROM, DATE_TO)
        results['sensors']['ICESat-2'] = ic_result
        print(f'    Found: {ic_result.get("count", 0)} scenes')
        
        # SWOT
        print('  SWOT SSH (Altimetry)...')
        swot_result = search_swot_cmr(target_data['bbox'], DATE_FROM, DATE_TO)
        results['sensors']['SWOT'] = swot_result
        print(f'    Found: {swot_result.get("count", 0)} scenes')
        
        all_results[target_name] = results
        print()
    
    # Summary
    print('='*80)
    print('COVERAGE SUMMARY')
    print('='*80)
    print()
    
    for target_name, results in all_results.items():
        print(f'{target_name}:')
        print()
        print(f'  {"Sensor":<20} {"Count":<8} {"Most Recent":<25}')
        print(f'  {"-"*55}')
        
        for sensor_name, sensor_data in results['sensors'].items():
            count = sensor_data.get('count', 0)
            scenes = sensor_data.get('scenes', [])
            
            if scenes:
                most_recent = scenes[0].get('datetime', 'N/A')[:19] if scenes else 'N/A'
            else:
                most_recent = 'N/A'
            
            print(f'  {sensor_name:<20} {count:<8} {most_recent}')
        print()
    
    # Save results
    output = {
        'search_date': datetime.now().isoformat(),
        'date_range': {'from': DATE_FROM, 'to': DATE_TO},
        'targets': all_results,
    }
    
    output_dir = Path('outputs/recent_coverage')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_json = output_dir / 'recent_satellite_coverage.json'
    with open(output_json, 'w') as f:
        json.dump(output, f, indent=2)
    
    print(f'Results saved: {output_json}')
    print()
    print('='*80)
    
    return all_results


if __name__ == '__main__':
    main()
