"""
Find aluminum candidates closest to South Haven, MI
"""

import json
from math import radians, cos, sin, asin, sqrt

# South Haven, MI coordinates
SOUTH_HAVEN = (42.4036, -86.2742)

def haversine(lat1, lon1, lat2, lon2):
    """Calculate distance between two points in km."""
    R = 6371
    lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
    c = 2 * asin(sqrt(a))
    return R * c

# Load candidates
d = json.load(open('c:/Users/thomf/programming/wreckhunter2000/outputs/aviation_filter/aviation_candidates_with_coords.json'))

# Calculate distance to South Haven for each candidate
for c in d['candidates']:
    if c.get('lat') and c.get('lon'):
        c['dist_km'] = haversine(c['lat'], c['lon'], SOUTH_HAVEN[0], SOUTH_HAVEN[1])
        c['dist_mi'] = c['dist_km'] * 0.621371

# Sort by distance
sorted_candidates = sorted([c for c in d['candidates'] if c.get('lat')], key=lambda x: x['dist_km'])

print('='*80)
print('ALUMINUM CANDIDATES - CLOSEST TO SOUTH HAVEN, MI')
print('='*80)
print(f'South Haven coordinates: {SOUTH_HAVEN[0]:.4f}N, {SOUTH_HAVEN[1]:.4f}W')
print()
print('TOP 10 CLOSEST:')
print()

for i, c in enumerate(sorted_candidates[:10], 1):
    print(f'{i:2d}. Band: {c["band"]:3s} | Mag: {c["magnitude"]:.4f} | Lat: {c["lat"]:.6f}N | Lon: {c["lon"]:.6f}W | Dist: {c["dist_km"]:.1f} km ({c["dist_mi"]:.1f} mi)')

print()
print('='*80)
print('CLOSEST CANDIDATE:')
closest = sorted_candidates[0]
overall_rank = sorted_candidates.index(closest) + 1
print(f'  Overall aluminum rank: #{overall_rank}')
print(f'  Band: {closest["band"]}')
print(f'  Magnitude: {closest["magnitude"]:.4f}')
print(f'  Coordinates: {closest["lat"]:.6f}N, {closest["lon"]:.6f}W')
print(f'  Distance from South Haven: {closest["dist_km"]:.1f} km ({closest["dist_mi"]:.1f} miles)')
print('='*80)
