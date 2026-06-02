"""Calculate distance from South Haven, MI to Wreck Alley"""
import numpy as np

# South Haven, MI (eastern shore of Lake Michigan)
south_haven = {'lat': 42.403, 'lon': -86.274}

# Wreck Alley / Andaste Cluster (western shore, Zion/Waukegan area)
wreck_alley = {'lat': 42.47, 'lon': -87.10}

# Haversine distance
lat1 = np.radians(south_haven['lat'])
lon1 = np.radians(south_haven['lon'])
lat2 = np.radians(wreck_alley['lat'])
lon2 = np.radians(wreck_alley['lon'])

dlat = lat2 - lat1
dlon = lon2 - lon1

a = np.sin(dlat/2)**2 + np.cos(lat1) * np.cos(lat2) * np.sin(dlon/2)**2
c = 2 * np.arcsin(np.sqrt(a))

distance_miles = 3959 * c
distance_km = 6371 * c
distance_nm = distance_miles / 1.151

print('='*60)
print('DISTANCE FROM SOUTH HAVEN, MI TO WRECK ALLEY')
print('='*60)
print()
print('FROM: South Haven, MI')
print(f'  Coordinates: {south_haven["lat"]:.2f}N, {abs(south_haven["lon"]):.2f}W')
print('  Location: Eastern shore of Lake Michigan')
print()
print('TO: Wreck Alley (Andaste Cluster)')
print(f'  Coordinates: {wreck_alley["lat"]:.2f}N, {abs(wreck_alley["lon"]):.2f}W')
print('  Location: Western shore (Zion/Waukegan area)')
print()
print('DISTANCE:')
print(f'  {distance_miles:.1f} miles (statute)')
print(f'  {distance_km:.1f} km')
print(f'  {distance_nm:.1f} nautical miles')
print()
print('INTERPRETATION:')
print('  - This is a FULL LAKE CROSSING (east to west shore)')
print(f'  - Lake Michigan width at this latitude: ~{distance_miles:.0f} miles')
print('  - Driving distance (around lake via I-94/I-96): ~250 miles')
print(f'  - Boat transit time (25 knots): ~{distance_nm/25:.1f} hours')
print(f'  - Boat transit time (15 knots): ~{distance_nm/15:.1f} hours')
print()
print('='*60)
