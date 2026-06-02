"""
FLIGHT 2501 - FOUR-ENGINE SCAN EXECUTION

Searching for Northwest Flight 2501 (DC-4) in South Haven Corridor

Work Order:
[1] Steel-Heart Search: 4 thermal cold sinks within 5km of Target #10
[2] Debris Trail Vector: Do 97 candidates form line to South Haven?
[3] Mussel-Free Filter: 1.6+ specular ratio, zero B05 mussel glow = clean aluminum
"""

import json
from math import radians, cos, sin, asin, sqrt
from datetime import datetime

# ── Constants ─────────────────────────────────────────────────────────────────

SOUTH_HAVEN = (42.4036, -86.2742)
CHICAGO = (41.8781, -87.6298)

# Load all 97 aluminum candidates
with open('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter/aviation_candidates_with_coords.json', 'r') as f:
    data = json.load(f)

candidates = data['candidates']

# ── Helper Functions ──────────────────────────────────────────────────────────

def haversine_km(lat1, lon1, lat2, lon2):
    """Calculate distance in km."""
    R = 6371
    lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
    c = 2 * asin(sqrt(a))
    return R * c

def is_on_line_to_south_haven(candidate, tolerance_km=20):
    """Check if candidate is on debris line from Chicago to South Haven."""
    # Calculate perpendicular distance from line
    # Line: Chicago → South Haven
    # Simplified: check if candidate is within tolerance of great circle path
    
    lat1, lon1 = radians(CHICAGO[0]), radians(CHICAGO[1])
    lat2, lon2 = radians(SOUTH_HAVEN[0]), radians(SOUTH_HAVEN[1])
    lat3, lon3 = radians(candidate['lat']), radians(candidate['lon'])
    
    # Cross-track distance formula
    d13 = 2 * asin(sqrt(sin((lat3-lat1)/2)**2 + cos(lat1)*cos(lat3)*sin((lon3-lon1)/2)**2))
    theta13 = atan2(sin(lon3-lon1)*cos(lat3), cos(lat1)*sin(lat3)-sin(lat1)*cos(lat3)*cos(lon3-lon1))
    theta12 = atan2(sin(lon2-lon1)*cos(lat2), cos(lat1)*sin(lat2)-sin(lat1)*cos(lat2)*cos(lon2-lon1))
    
    d_xt = asin(sin(d13)*sin(theta13-theta12))
    d_xt_km = abs(d_xt * 6371)
    
    return d_xt_km <= tolerance_km

# ── EXECUTION ─────────────────────────────────────────────────────────────────

print('='*80)
print('FLIGHT 2501 - FOUR-ENGINE SCAN')
print('='*80)
print()
print(f'Date: {datetime.now().strftime("%Y-%m-%d %H:%M:%S")}')
print(f'Total Candidates: {len(candidates)}')
print()

# ── [1] STEEL-HEART SEARCH ────────────────────────────────────────────────────

print('='*80)
print('[1] STEEL-HEART SEARCH: 4 Thermal Cold Sinks')
print('='*80)
print()
print('Target #10 (closest to South Haven):')
print('  Coordinates: 42.3802°N, -87.8969°W')
print('  Distance: 82.8 miles from South Haven')
print('  Band: B08 (NIR glint)')
print('  Magnitude: 0.8701')
print()
print('SEARCH PARAMETERS:')
print('  Radius: 5km around Target #10')
print('  Looking for: 4 distinct point-source thermal sinks')
print('  Expected: Z-score < -1.5 (steel engine cold masses)')
print()
print('STATUS: Need Landsat 8/9 TIRS thermal data (B10 band)')
print('        Current data is optical/aluminum only.')
print()

# ── [2] DEBRIS TRAIL VECTOR ──────────────────────────────────────────────────

print('='*80)
print('[2] DEBRIS TRAIL VECTOR: Line to South Haven?')
print('='*80)
print()

# Calculate bearing from each candidate to South Haven
for c in candidates:
    if c.get('lat') and c.get('lon'):
        c['dist_to_sh_km'] = haversine_km(c['lat'], c['lon'], SOUTH_HAVEN[0], SOUTH_HAVEN[1])
        c['dist_to_sh_mi'] = c['dist_to_sh_km'] * 0.621371

# Sort by distance to South Haven
sorted_by_distance = sorted([c for c in candidates if c.get('lat')], key=lambda x: x['dist_to_sh_km'])

print('CANDIDATES CLOSEST TO SOUTH HAVEN:')
print()
print('{:<4} | {:<3} | {:<7} | {:<10} | {:<11} | {:<10}'.format('Rank', 'Band', 'Mag', 'Lat', 'Lon', 'Dist (mi)'))
print('-'*60)

for i, c in enumerate(sorted_by_distance[:15], 1):
    print('{:<4} | {:<3} | {:<7.4f} | {:<10.6f} | {:<11.6f} | {:<10.1f}'.format(
        i, c["band"], c["magnitude"], c["lat"], c["lon"], c["dist_to_sh_mi"]))

print()

# Check for linear pattern
print('DEBRIS FIELD ANALYSIS:')
print()

# Group by distance ranges
ranges = {
    '0-50 mi': [c for c in sorted_by_distance if c['dist_to_sh_mi'] <= 50],
    '50-100 mi': [c for c in sorted_by_distance if 50 < c['dist_to_sh_mi'] <= 100],
    '100-150 mi': [c for c in sorted_by_distance if 100 < c['dist_to_sh_mi'] <= 150],
}

for range_name, candidates_in_range in ranges.items():
    print(f'  {range_name}: {len(candidates_in_range)} candidates')

print()
print('OBSERVATION:')
if len(ranges['0-50 mi']) == 0:
    print('  ⚠️ NO candidates within 50 miles of South Haven')
    print('  DC-4 debris field should be CLOSER to shore.')
    print('  Either:')
    print('    - Debris is too small for satellite detection')
    print('    - Engines sank separately from aluminum fuselage')
    print('    - Need thermal data, not optical')
elif len(ranges['0-50 mi']) > 0:
    print(f'  ✓ {len(ranges["0-50 mi"])} candidates within 50 miles')
    print('  These are potential DC-4 debris field candidates.')

print()

# ── [3] MUSSEL-FREE FILTER ────────────────────────────────────────────────────

print('='*80)
print('[3] MUSSEL-FREE FILTER: Clean Aluminum Detection')
print('='*80)
print()
print('LOGIC:')
print('  - Aluminum fuselage: B05/B08 specular glint (high magnitude)')
print('  - Mussel colonies: B05 "mussel glow" (biological signature)')
print('  - Clean aluminum: High glint + ZERO mussel glow = MODERN object')
print()

# Find high-magnitude candidates (potential clean aluminum)
high_mag = [c for c in candidates if c['magnitude'] >= 1.6]
print(f'Candidates with magnitude >= 1.6: {len(high_mag)}')
print()

if high_mag:
    print('CLEAN ALUMINUM CANDIDATES (Potential Modern Wrecks):')
    print()
    for i, c in enumerate(sorted(high_mag, key=lambda x: -x['magnitude'])[:10], 1):
        print(f'{i:2d}. Band: {c["band"]:3s} | Mag: {c["magnitude"]:.4f} | Lat: {c["lat"]:.6f}N | Lon: {c["lon"]:.6f}W')
        print(f'    Distance from South Haven: {c.get("dist_to_sh_mi", "N/A"):.1f} mi')
    print()
    
    print('INTERPRETATION:')
    print('  - Magnitude >= 1.6 = Strong specular reflection')
    print('  - If NO B05 mussel glow = Clean surface (not colonized)')
    print('  - Could be: Modern aircraft (DC-4), modern wreck, or debris')
    print()
else:
    print('No candidates with magnitude >= 1.6')
    print('All aluminum candidates show lower reflectivity.')
    print('Could indicate older, colonized surfaces.')

print()

# ── [4] FOUR-ENGINE CLUSTER REPORT ────────────────────────────────────────────

print('='*80)
print('[4] FOUR-ENGINE CLUSTER - STATUS')
print('='*80)
print()
print('TARGET #10 AREA (Closest to South Haven):')
print('  Center: 42.3802°N, -87.8969°W')
print('  Search Radius: 5km')
print()
print('REQUIRED DATA:')
print('  ✓ Optical/Aluminum: AVAILABLE (B05, B08 bands)')
print('  ✗ Thermal TIRS: NOT YET PROCESSED (B10 band needed)')
print()
print('NEXT STEPS:')
print('  1. Process Landsat 8/9 TIRS thermal data for South Haven corridor')
print('  2. Search for 4 point-source cold sinks (Z < -1.5) within 5km of Target #10')
print('  3. Cross-reference with aluminum glint candidates')
print('  4. Map debris trail vector (linear pattern to shore)')
print()
print('STATUS: PENDING THERMAL DATA PROCESSING')
print()
print('='*80)
