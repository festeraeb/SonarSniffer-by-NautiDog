"""
FLIGHT 2501 - CORRECTED MID-LAKE ALUMINUM SCAN

ERROR CORRECTION: Previous coordinates (-87.96°W to -88.06°W) were ON LAND (Wisconsin).
CORRECT AREA: Mid-lake between South Haven, MI and Wisconsin shore.

SEARCH AREA:
- Latitude: 42.0°N to 43.5°N (South Haven latitude range)
- Longitude: -86.5°W to -87.0°W (MID-LAKE, not western shore)
- Distance from South Haven: 30-60 miles (not 95 miles)

This scans for aluminum debris from DC-4 that would have fallen in MID-LAKE,
not on the Wisconsin shore.
"""

import json
from pathlib import Path
from datetime import datetime

# ── CORRECTED SEARCH AREA ─────────────────────────────────────────────────────

# Expanded search area - includes closer to shore AND mid-lake
MID_LAKE_BBOX = {
    'lat_min': 42.0,
    'lat_max': 43.5,
    'lon_min': -87.2,  # Western boundary (mid-lake, not Wisconsin shore)
    'lon_max': -86.3,  # Eastern boundary (closer to Michigan shore)
}

print('='*80)
print('FLIGHT 2501 - CORRECTED MID-LAKE ALUMINUM SCAN')
print('='*80)
print()
print(f'Analysis Date: {datetime.now().strftime("%Y-%m-%d %H:%M:%S")}')
print()
print('ERROR CORRECTION:')
print('  Previous coords (-87.96°W to -88.06°W) were ON LAND (Wisconsin)')
print('  Correct area: Mid-lake (-86.5°W to -87.0°W)')
print()
print('CORRECTED SEARCH AREA:')
print(f'  Latitude: {MID_LAKE_BBOX["lat_min"]:.1f}°N to {MID_LAKE_BBOX["lat_max"]:.1f}°N')
print(f'  Longitude: {MID_LAKE_BBOX["lon_min"]:.1f}°W to {MID_LAKE_BBOX["lon_max"]:.1f}°W')
print(f'  Distance from South Haven: 20-60 miles (shore to mid-lake)')
print()

# Load existing aluminum candidates
aviation_file = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter/aviation_candidates_with_coords.json')

if not aviation_file.exists():
    print('STATUS: No existing aluminum candidates file found.')
    print('Need to run aviation aluminum filter first.')
else:
    with open(aviation_file, 'r') as f:
        data = json.load(f)
    
    all_candidates = data.get('candidates', [])
    
    # Filter for mid-lake area
    mid_lake_candidates = []
    for c in all_candidates:
        if c.get('lat') and c.get('lon'):
            lat = c['lat']
            lon = c['lon']
            # Check if in corrected mid-lake area
            if (MID_LAKE_BBOX['lat_min'] <= lat <= MID_LAKE_BBOX['lat_max'] and
                MID_LAKE_BBOX['lon_max'] <= lon <= MID_LAKE_BBOX['lon_min']):
                mid_lake_candidates.append(c)
    
    print(f'Total aluminum candidates (all areas): {len(all_candidates)}')
    print(f'Candidates in CORRECTED mid-lake area: {len(mid_lake_candidates)}')
    print()
    
    if mid_lake_candidates:
        print('MID-LAKE CANDIDATES (Potential Flight 2501 Debris):')
        print()
        print('{:<4} | {:<6} | {:<7} | {:<10} | {:<11} | {:<8}'.format(
            'Rank', 'Band', 'Mag', 'Lat', 'Lon', 'Dist SH'))
        print('-'*60)
        
        # Sort by magnitude (highest first)
        sorted_candidates = sorted(mid_lake_candidates, key=lambda x: -x['magnitude'])
        
        for i, c in enumerate(sorted_candidates[:20], 1):
            # Calculate distance from South Haven
            from math import radians, cos, sin, asin, sqrt
            south_haven = (42.4036, -86.2742)
            R = 3959
            lat1, lon1 = radians(south_haven[0]), radians(south_haven[1])
            lat2, lon2 = radians(c['lat']), radians(c['lon'])
            dlat = lat2 - lat1
            dlon = lon2 - lon1
            a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
            c = 2 * asin(sqrt(a))
            dist_mi = R * c
            
            print('{:<4} | {:<6} | {:<7.4f} | {:<10.6f} | {:<11.6f} | {:<8.1f} mi'.format(
                i, c['band'], c['magnitude'], c['lat'], c['lon'], dist_mi))
        
        print()
        print(f'Showing top {min(20, len(sorted_candidates))} of {len(sorted_candidates)} mid-lake candidates')
        print()
        
        if len(sorted_candidates) >= 4:
            print('TOP 4 CANDIDATES (Potential Engine Locations):')
            for i, c in enumerate(sorted_candidates[:4], 1):
                print(f'  Engine #{i}: {c["lat"]:.6f}N, {c["lon"]:.6f}W (Mag {c["magnitude"]:.4f}, {c["band"]})')
        print()
    else:
        print('⚠️  NO aluminum candidates found in corrected mid-lake area.')
        print()
        print('POSSIBLE EXPLANATIONS:')
        print('  1. Debris field is smaller than satellite resolution')
        print('  2. Aluminum has corroded/dispersed in 75 years')
        print('  3. Need to expand search area')
        print('  4. Flight 2501 may have gone down closer to shore')
        print()

print('='*80)
print('NEXT STEPS:')
print('='*80)
print()
print('1. ✅ Corrected search area to mid-lake (not Wisconsin shore)')
print('2. ⏳ Process thermal data (B10 TIRS) for mid-lake candidates')
print('3. ⏳ Cross-reference with historical flight path data')
print('4. ⏳ Expand search if no candidates found')
print()
print('='*80)
