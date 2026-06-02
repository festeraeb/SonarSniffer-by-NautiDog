"""
sar_stac_query.py

Query Sentinel-1 SAR via STAC API (alternative to ASF Vertex)

This uses the Earth-search STAC API for Sentinel-1 GRD products.
"""

import json
import requests
from pathlib import Path
from datetime import datetime

# STAC API for Sentinel-1
STAC_API = 'https://earth-search.aws.element84.com/v1/search'

# Andaste corridor bbox
BBOX = [-87.15, 42.40, -87.05, 42.55]  # lon_min, lat_min, lon_max, lat_max

# Date range
DATETIME = '2020-01-01T00:00:00Z/2025-12-31T23:59:59Z'

print('='*80)
print('SENTINEL-1 SAR STAC QUERY - ANDASTE CORRIDOR')
print('='*80)
print()

# STAC query
payload = {
    'collections': ['sentinel-1-grd'],
    'bbox': BBOX,
    'datetime': DATETIME,
    'query': {
        'sar:product_type': {'eq': 'GRD'},
        'sar:polarizations': {'contains': 'VV'},
    },
    'limit': 50,
}

print(f'Querying STAC API...')
print(f'  Bbox: {BBOX}')
print(f'  Date: {DATETIME}')
print()

try:
    resp = requests.post(STAC_API, json=payload, timeout=60)
    resp.raise_for_status()
    
    data = resp.json()
    features = data.get('features', [])
    
    print(f'Results: {len(features)} Sentinel-1 scenes')
    print()
    
    if features:
        print('Available scenes:')
        print('-'*80)
        
        for i, feat in enumerate(features[:20], 1):
            props = feat.get('properties', {})
            
            scene_id = feat.get('id', 'unknown')
            date = props.get('datetime', props.get('start_datetime', 'unknown'))
            polarizations = props.get('sar:polarizations', [])
            orbit = props.get('sat:relative_orbit', props.get('mgrs:utm_zone', 'unknown'))
            
            # Get asset URLs
            assets = feat.get('assets', {})
            asset_keys = list(assets.keys())
            
            print(f'{i:2}. {date[:10] if date else "unknown":12} | Orbit {orbit:5} | Pol: {polarizations}')
            print(f'    Assets: {", ".join(asset_keys[:5])}')
            print()
        
        if len(features) > 20:
            print(f'... and {len(features) - 20} more scenes')
        
        print()
        print('='*80)
        print('SUMMARY')
        print('='*80)
        print()
        print(f'Total Sentinel-1 scenes over Andaste corridor: {len(features)}')
        print()
        print('STATUS: ✅ SAR DATA AVAILABLE')
        print()
        print('NEXT STEPS:')
        print('  1. Download GeoTIFFs from asset URLs')
        print('  2. Extract sigma0 at target coordinates')
        print('  3. Apply Nauticuvs curvelets for edge enhancement')
        print('  4. Compute temporal coherence')
        print()
        
        # Save scene list
        output = {
            'query_date': datetime.now().isoformat(),
            'bbox': BBOX,
            'datetime_range': DATETIME,
            'scenes_found': len(features),
            'scenes': [
                {
                    'id': f.get('id'),
                    'datetime': f.get('properties', {}).get('datetime'),
                    'orbit': f.get('properties', {}).get('sat:relative_orbit'),
                    'polarizations': f.get('properties', {}).get('sar:polarizations'),
                    'assets': list(f.get('assets', {}).keys()),
                }
                for f in features
            ]
        }
        
        output_path = Path('outputs/sar_nauticuvs_andaste/sentinel1_stac_results.json')
        output_path.parent.mkdir(parents=True, exist_ok=True)
        with open(output_path, 'w') as f:
            json.dump(output, f, indent=2)
        
        print(f'Results saved: {output_path}')
        
    else:
        print('[!] No Sentinel-1 scenes found in STAC')
        print()
        print('Possible reasons:')
        print('  1. No VV polarization coverage in this area')
        print('  2. STAC collection name may differ')
        print('  3. API temporarily unavailable')
        
except Exception as e:
    print(f'[!] STAC query failed: {e}')
    print()
    print('Trying alternative: ESA Copernicus Data Space...')
