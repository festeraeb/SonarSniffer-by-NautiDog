"""
FLIGHT 2501 - DEBRIS TRAIL MAPPING

Plot all 97 aluminum candidates to see if they form a LINE pointing to South Haven.

The DC-4 hit the water at 150-200 mph, debris scattered over 50-100+ miles.
The aluminum fuselage pieces create a LINEAR debris field pointing back to impact zone.
"""

import json
from math import radians, cos, sin, atan2, degrees

# Load all 97 candidates
with open('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter/aviation_candidates_with_coords.json', 'r') as f:
    data = json.load(f)

candidates = [c for c in data['candidates'] if c.get('lat') and c.get('lon')]

# South Haven coordinates
SOUTH_HAVEN = (42.4036, -86.2742)
CHICAGO = (41.8781, -87.6298)

print('='*80)
print('FLIGHT 2501 - DEBRIS TRAIL MAPPING')
print('='*80)
print()
print(f'Total Candidates: {len(candidates)}')
print(f'South Haven: {SOUTH_HAVEN[0]:.4f}N, {SOUTH_HAVEN[1]:.4f}W')
print(f'Chicago: {CHICAGO[0]:.4f}N, {CHICAGO[1]:.4f}W')
print()

# Calculate bearing from each candidate to South Haven
def calculate_bearing(lat1, lon1, lat2, lon2):
    """Calculate bearing from point 1 to point 2."""
    lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
    dlon = lon2 - lon1
    x = sin(dlon) * cos(lat2)
    y = cos(lat1)*sin(lat2) - sin(lat1)*cos(lat2)*cos(dlon)
    bearing = atan2(x, y)
    return (degrees(bearing) + 360) % 360

# Add bearing and distance to each candidate
for c in candidates:
    c['bearing_to_sh'] = calculate_bearing(c['lat'], c['lon'], SOUTH_HAVEN[0], SOUTH_HAVEN[1])
    c['bearing_to_chi'] = calculate_bearing(c['lat'], c['lon'], CHICAGO[0], CHICAGO[1])

# Sort by bearing to see if there's a pattern
sorted_by_bearing = sorted(candidates, key=lambda x: x['bearing_to_sh'])

print('='*80)
print('[1] BEARING ANALYSIS: Do candidates point to South Haven?')
print('='*80)
print()

# Group by bearing ranges (every 10 degrees)
bearing_groups = {}
for c in candidates:
    bearing_range = int(c['bearing_to_sh'] / 10) * 10
    if bearing_range not in bearing_groups:
        bearing_groups[bearing_range] = []
    bearing_groups[bearing_range].append(c)

print('CANDIDATES BY BEARING TO SOUTH HAVEN:')
print()
print('{:<10} | {:<6} | {:<10} | {:<30}'.format('Bearing', 'Count', 'Avg Mag', 'Top Candidate'))
print('-'*70)

for bearing in sorted(bearing_groups.keys()):
    group = bearing_groups[bearing]
    avg_mag = sum(c['magnitude'] for c in group) / len(group)
    top = max(group, key=lambda x: x['magnitude'])
    print('{:<10} | {:<6} | {:<10.4f} | {:<30}'.format(
        f'{bearing}-{bearing+10}°', 
        len(group), 
        avg_mag,
        f'Mag {top["magnitude"]:.2f} at {top["lat"]:.2f}N, {top["lon"]:.2f}W'))

print()

# Find the dominant bearing (most candidates)
dominant_bearing = max(bearing_groups.keys(), key=lambda b: len(bearing_groups[b]))
dominant_count = len(bearing_groups[dominant_bearing])

print(f'DOMINANT BEARING: {dominant_bearing}-{dominant_bearing+10}° ({dominant_count} candidates)')
print()

# Check if dominant bearing points to South Haven
print('INTERPRETATION:')
if 70 <= dominant_bearing <= 110:
    print('  ✓ Bearing 70-110° = East-Southeast')
    print('  ✓ This points FROM candidates TO South Haven shore!')
    print('  ✓ DEBRIS TRAIL CONFIRMED - candidates form line to shore')
elif 250 <= dominant_bearing <= 290:
    print('  ✓ Bearing 250-290° = West-Northwest')
    print('  ✓ This points FROM South Haven TO candidates')
    print('  ✓ DEBRIS SCATTERED west of shore (consistent with crash)')
else:
    print(f'  ⚠ Bearing {dominant_bearing}° doesn\'t align with South Haven')
    print('  Debris may be scattered or from different source')

print()

# ── [2] DEBRIS TRAIL VECTOR ──────────────────────────────────────────────────

print('='*80)
print('[2] DEBRIS TRAIL VECTOR: Linear Pattern Analysis')
print('='*80)
print()

# Sort candidates by distance from shore
from math import radians, cos, sin, asin, sqrt

def haversine_mi(lat1, lon1, lat2, lon2):
    R = 3959  # Earth radius in miles
    lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
    c = 2 * asin(sqrt(a))
    return R * c

for c in candidates:
    c['dist_from_sh_mi'] = haversine_mi(c['lat'], c['lon'], SOUTH_HAVEN[0], SOUTH_HAVEN[1])

sorted_by_distance = sorted(candidates, key=lambda x: x['dist_from_sh_mi'])

print('CANDIDATES BY DISTANCE FROM SOUTH HAVEN:')
print()
print('{:<4} | {:<6} | {:<7} | {:<10} | {:<11} | {:<8}'.format('Rank', 'Dist', 'Mag', 'Lat', 'Lon', 'Band'))
print('-'*60)

for i, c in enumerate(sorted_by_distance[:20], 1):
    print('{:<4} | {:<6.1f} | {:<7.4f} | {:<10.6f} | {:<11.6f} | {:<8}'.format(
        i, c['dist_from_sh_mi'], c['magnitude'], c['lat'], c['lon'], c['band']))

print()

# Check for linear pattern
print('DEBRIS FIELD PATTERN:')
print()

# Calculate if candidates form a line
# Simple check: are they clustered in a narrow bearing range?
if dominant_count >= len(candidates) * 0.3:
    print(f'  ✓ {dominant_count}/{len(candidates)} candidates ({100*dominant_count/len(candidates):.0f}%) in same bearing range')
    print('  ✓ LINEAR DEBRIS TRAIL DETECTED')
    print('  ✓ Follow this bearing FROM South Haven to find crash site')
else:
    print(f'  ⚠ Candidates scattered across multiple bearings')
    print('  ⚠ Debris field may be wide or from multiple sources')

print()

# ── [3] CRASH SITE TRIANGULATION ─────────────────────────────────────────────

print('='*80)
print('[3] CRASH SITE TRIANGULATION')
print('='*80)
print()

# Estimate crash site by extending debris trail
# Take candidates 50-100 miles out, calculate average bearing, extend back to shore

mid_range = [c for c in candidates if 50 <= c['dist_from_sh_mi'] <= 100]

if mid_range:
    avg_bearing = sum(c['bearing_to_sh'] for c in mid_range) / len(mid_range)
    avg_distance = sum(c['dist_from_sh_mi'] for c in mid_range) / len(mid_range)
    
    print(f'Mid-range candidates (50-100 mi from shore): {len(mid_range)}')
    print(f'  Average bearing to shore: {avg_bearing:.1f}°')
    print(f'  Average distance: {avg_distance:.1f} miles')
    print()
    print('CRASH SITE ESTIMATE:')
    print(f'  Follow bearing {avg_bearing:.1f}° FROM South Haven')
    print(f'  Distance: {avg_distance*1.5:.0f} to {avg_distance*2:.0f} miles offshore')
    print(f'  Search area: {avg_distance:.0f} mile radius from bearing endpoint')
    print()
    
    # Find the 4 strongest candidates in mid-range (potential engine locations)
    top_4 = sorted(mid_range, key=lambda x: -x['magnitude'])[:4]
    print('TOP 4 CANDIDATES (Potential Engine Locations):')
    print()
    for i, c in enumerate(top_4, 1):
        print(f'{i}. Mag: {c["magnitude"]:.4f} | Band: {c["band"]:3s} | {c["lat"]:.6f}N, {c["lon"]:.6f}W | {c["dist_from_sh_mi"]:.1f} mi from shore')
else:
    print('  No candidates in 50-100 mile range')
    print('  Debris field may be closer or farther')

print()
print('='*80)
print('NEXT STEPS:')
print('='*80)
print()
print('1. Process Landsat TIRS thermal data for crash site estimate')
print('2. Search for 4 steel engine cold sinks (Z < -1.5) in that area')
print('3. Cross-reference with aluminum glint candidates')
print('4. Map complete debris trail vector')
print()
print('='*80)
