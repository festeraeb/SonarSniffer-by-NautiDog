"""
ice_free_coverage_search.py

Search for ice-free satellite coverage over Milwaukee corridor.

Great Lakes ice season: January - April (peak: Feb-March)
Ice-free periods: May - December (best: Aug-Oct)

Search for:
1. Pre-January 2026 data (Aug-Dec 2025) - likely ice-free
2. Check ice concentration maps for Feb 2026 scenes
3. Find optimal clear-sky + ice-free windows
"""

import json
import requests
from datetime import datetime
from pathlib import Path

# ── Target Area ──────────────────────────────────────────────────────────────

MILWAUKEE_CORRIDOR = {
    'name': 'Milwaukee 7-Mile Corridor',
    'bbox': [-87.95, 42.49, -87.80, 43.05],  # lon_min, lat_min, lon_max, lat_max
}

# ── Search Periods ────────────────────────────────────────────────────────────

# Ice-free period (Aug-Dec 2025)
ICE_FREE_PERIODS = [
    {'name': 'Fall 2025 (Ice-Free)', 'from': '2025-08-25T00:00:00Z', 'to': '2025-12-15T23:59:59Z'},
    {'name': 'Late Winter 2025 (Ice Peak)', 'from': '2025-02-01T00:00:00Z', 'to': '2025-03-31T23:59:59Z'},
    {'name': 'Spring 2025 (Ice Melt)', 'from': '2025-04-01T00:00:00Z', 'to': '2025-05-31T23:59:59Z'},
]

# STAC API
STAC_API = 'https://earth-search.aws.element84.com/v1/search'

# ── Search Functions ──────────────────────────────────────────────────────────

def search_sentinel2_period(bbox: list, date_from: str, date_to: str, max_cloud: float = 20) -> dict:
    """Search Sentinel-2 with cloud cover filter"""
    payload = {
        'collections': ['sentinel-2-l2a'],
        'bbox': bbox,
        'datetime': f'{date_from}/{date_to}',
        'query': {
            'eo:cloud_cover': {'lte': max_cloud},
        },
        'limit': 100,
    }
    
    try:
        resp = requests.post(STAC_API, json=payload, timeout=60)
        resp.raise_for_status()
        data = resp.json()
        features = data.get('features', [])
        
        return {
            'count': len(features),
            'scenes': [
                {
                    'id': f.get('id'),
                    'datetime': f.get('properties', {}).get('datetime')[:10],
                    'cloud_cover': f.get('properties', {}).get('eo:cloud_cover'),
                }
                for f in features[:30]
            ]
        }
    except Exception as e:
        return {'error': str(e), 'count': 0, 'scenes': []}


def search_sentinel1_period(bbox: list, date_from: str, date_to: str) -> dict:
    """Search Sentinel-1 SAR (not affected by clouds/ice)"""
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
            'count': len(features),
            'scenes': [
                {
                    'id': f.get('id'),
                    'datetime': f.get('properties', {}).get('datetime')[:10],
                    'orbit': f.get('properties', {}).get('sat:relative_orbit'),
                }
                for f in features[:30]
            ]
        }
    except Exception as e:
        return {'error': str(e), 'count': 0, 'scenes': []}


# ── Main Search ───────────────────────────────────────────────────────────────

def main():
    print('='*80)
    print('ICE-FREE COVERAGE SEARCH - MILWAUKEE CORRIDOR')
    print('Great Lakes Ice Analysis')
    print('='*80)
    print()
    
    # Great Lakes ice info
    print('GREAT LAKES ICE SEASONALITY:')
    print('-'*60)
    print('  Ice Formation: December - January')
    print('  Peak Ice: February - March (up to 80% coverage)')
    print('  Ice Melt: April - May')
    print('  Ice-Free: June - November (optimal: Aug-Oct)')
    print()
    print('  Milwaukee Corridor (7 miles offshore):')
    print('    - Near-shore ice: Jan-May')
    print('    - Open water: 7-mile depth = ~300ft, slower to freeze')
    print('    - But surface ice still possible Feb-Mar')
    print()
    
    all_results = {}
    
    for period in ICE_FREE_PERIODS:
        print('='*80)
        print(f"PERIOD: {period['name']}")
        print(f"  {period['from'][:10]} to {period['to'][:10]}")
        print('='*80)
        print()
        
        results = {
            'period': period['name'],
            'date_range': {'from': period['from'], 'to': period['to']},
            'sensors': {}
        }
        
        # Sentinel-2 (optical - affected by clouds AND ice)
        print('  Sentinel-2 L2A (Optical, <20% cloud)...')
        s2_result = search_sentinel2_period(
            MILWAUKEE_CORRIDOR['bbox'],
            period['from'],
            period['to'],
            max_cloud=20
        )
        results['sensors']['Sentinel-2'] = s2_result
        print(f'    Found: {s2_result.get("count", 0)} scenes')
        
        if s2_result.get('scenes'):
            print('    Best dates:')
            for scene in s2_result['scenes'][:5]:
                print(f"      {scene['datetime']}: {scene['cloud_cover']:.1f}% cloud")
        
        # Sentinel-1 (SAR - NOT affected by clouds or ice)
        print('  Sentinel-1 GRD (SAR - sees through ice/clouds)...')
        s1_result = search_sentinel1_period(
            MILWAUKEE_CORRIDOR['bbox'],
            period['from'],
            period['to']
        )
        results['sensors']['Sentinel-1'] = s1_result
        print(f'    Found: {s1_result.get("count", 0)} scenes')
        
        if s1_result.get('scenes'):
            print('    Sample dates:')
            for scene in s1_result['scenes'][:5]:
                print(f"      {scene['datetime']}: Orbit {scene['orbit']}")
        
        all_results[period['name']] = results
        print()
    
    # Summary
    print('='*80)
    print('COVERAGE SUMMARY BY PERIOD')
    print('='*80)
    print()
    
    print(f'{"Period":<35} {"S2 (<20% cloud)":<18} {"S1 (SAR)":<12}')
    print('-'*65)
    
    for period_name, results in all_results.items():
        s2_count = results['sensors'].get('Sentinel-2', {}).get('count', 0)
        s1_count = results['sensors'].get('Sentinel-1', {}).get('count', 0)
        print(f'{period_name:<35} {s2_count:<18} {s1_count:<12}')
    
    print()
    
    # Recommendations
    print('='*80)
    print('RECOMMENDATIONS')
    print('='*80)
    print()
    
    # Find best period
    best_optical = max(all_results.items(), 
                       key=lambda x: x[1]['sensors'].get('Sentinel-2', {}).get('count', 0))
    best_sar = max(all_results.items(),
                   key=lambda x: x[1]['sensors'].get('Sentinel-1', {}).get('count', 0))
    
    print(f'BEST OPTICAL (Sentinel-2): {best_optical[0]}')
    print(f'  - {best_optical[1]["sensors"]["Sentinel-2"]["count"]} scenes with <20% cloud')
    print()
    
    print(f'BEST SAR (Sentinel-1): {best_sar[0]}')
    print(f'  - {best_sar[1]["sensors"]["Sentinel-1"]["count"]} scenes')
    print()
    
    print('ICE CONSIDERATIONS:')
    print('  - Fall 2025 (Aug-Dec): ICE-FREE, optimal for optical')
    print('  - Feb 2026: Likely ICE-COVERED (use SAR only)')
    print('  - SAR penetrates ice - can detect keels regardless')
    print('  - Optical requires ice-free + clear sky')
    print()
    
    print('PRIORITY PROCESSING:')
    print('  1. SAR: Process ALL dates (works through ice/clouds)')
    print('  2. Optical: Focus on Aug-Nov 2025 (ice-free period)')
    print('  3. Avoid: Feb-Mar optical (ice + clouds)')
    print()
    
    # Save results
    output = {
        'search_date': datetime.now().isoformat(),
        'target': MILWAUKEE_CORRIDOR['name'],
        'periods': all_results,
        'recommendations': {
            'best_optical': best_optical[0],
            'best_sar': best_sar[0],
        }
    }
    
    output_dir = Path('outputs/ice_free_coverage')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    output_json = output_dir / 'ice_free_coverage_results.json'
    with open(output_json, 'w') as f:
        json.dump(output, f, indent=2)
    
    print(f'Results saved: {output_json}')
    print()
    print('='*80)
    
    return all_results


if __name__ == '__main__':
    main()
