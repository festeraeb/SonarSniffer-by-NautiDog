"""
Convert Wreck Alley coordinates to UTM Zone 16N
"""
from pyproj import Transformer

# Wreck Alley targets (WGS84 lat/lon)
TARGETS = {
    'Target_1_Main': {
        'name': 'SS William H. Squire',
        'lat': 42.4729,
        'lon': -87.0970,
    },
    'Target_4_Broken': {
        'name': 'SS Andaste',
        'lat': 42.4675,
        'lon': -87.0813,
    },
    'Anchor_1': {
        'name': 'SS L.C. Kowalski (Main)',
        'lat': 42.464696,
        'lon': -87.108232,
    },
    'Anchor_3': {
        'name': 'SS L.C. Kowalski (Debris)',
        'lat': 42.470330,
        'lon': -87.098963,
    },
}

# Transformer: WGS84 to UTM Zone 16N (covers Lake Michigan)
# EPSG:32616 = WGS84 / UTM Zone 16N
transformer = Transformer.from_crs('EPSG:4326', 'EPSG:32616', always_xy=True)

print('='*90)
print('WRECK ALLEY - UTM COORDINATES')
print('='*90)
print()
print('Datum: WGS84')
print('UTM Zone: 16N (EPSG:32616)')
print('Location: Lake Michigan, Zion/Waukegan Corridor')
print()
print('-'*90)
print(f'{"Target":<25} {"Identity":<25} {"Easting (m)":<15} {"Northing (m)":<15} {"Zone"}')
print('-'*90)

utm_coords = {}

for target_id, data in TARGETS.items():
    easting, northing = transformer.transform(data['lon'], data['lat'])
    utm_coords[target_id] = {
        'easting': easting,
        'northing': northing,
        'zone': '16N',
        'datum': 'WGS84',
    }
    print(f'{target_id:<25} {data["name"]:<25} {easting:<15.1f} {northing:<15.1f} 16N')

print('-'*90)
print()
print('INDIVIDUAL COORDINATES (for GPS/Chart Plotter):')
print()

for target_id, data in TARGETS.items():
    utm = utm_coords[target_id]
    print(f'{target_id} ({data["name"]}):')
    print(f'  UTM: {utm["easting"]:.1f}E {utm["northing"]:.1f}N Zone {utm["zone"]}')
    print(f'  WGS84: {data["lat"]:.6f}N, {abs(data["lon"]):.6f}W')
    print()

print('='*90)
print('Note: UTM Zone 16N covers 90°W to 84°W (Lake Michigan)')
print('      Central Meridian: 87°W (runs through Lake Michigan)')
print('='*90)
