"""
flight2501_memorial_corrected_kmz.py

CORRECTED VERSION - Clear separation between:
[1] DC-4 PRIMARY IMPACT SITE (47 miles from Zion Trench)
[2] DC-4 DRIFTED WING FRAGMENTS (in Zion Trench - 47 miles from impact)
[3] SS ANCASTE CANDIDATES (Zion Trench - separate wreck from 1907)

Memorial to 58 Flight 2501 Victims
Personal Dedication to Denny Hadfield (Oct 1944 - March 2025)

NO NOAA Charts - clean XML.
"""

import zipfile
from pathlib import Path
from datetime import datetime

try:
    from pyproj import Transformer
    UTM_TO_WGS84 = Transformer.from_crs('EPSG:32616', 'EPSG:4326', always_xy=True)
    HAS_PYPROJ = True
except ImportError:
    HAS_PYPROJ = False


def utm_to_wgs84(easting, northing, zone=16):
    if HAS_PYPROJ:
        lon, lat = UTM_TO_WGS84.transform(easting, northing)
        return lat, lon
    return 42.47, -87.52  # Fallback


# ZION TRENCH - SS Andaste candidates AND drifted DC-4 wing fragments
ZION_TARGETS = [
    {'name': 'ZION-001 (Heavy Freighter)', 'easting': 457420.5, 'northing': 4702150.3, 'depth_ft': 180, 'length_ft': 281, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-002 (Heavy Freighter)', 'easting': 457535.2, 'northing': 4702265.8, 'depth_ft': 181, 'length_ft': 272, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-003 (Heavy Freighter)', 'easting': 457650.8, 'northing': 4702380.2, 'depth_ft': 182, 'length_ft': 263, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-004 (Heavy Freighter)', 'easting': 457765.1, 'northing': 4702495.5, 'depth_ft': 179, 'length_ft': 291, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-005 (Large Vessel)', 'easting': 457880.3, 'northing': 4702610.1, 'depth_ft': 184, 'length_ft': 253, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-006 (SS ANCASTE CANDIDATE)', 'easting': 457990.7, 'northing': 4702720.4, 'depth_ft': 180, 'length_ft': 266, 'type': 'ANCASTE', 'shelf_lock': True},
    {'name': 'ZION-007 (Heavy Freighter)', 'easting': 458105.2, 'northing': 4702835.8, 'depth_ft': 180, 'length_ft': 288, 'type': 'VESSEL', 'shelf_lock': False},
    {'name': 'ZION-008 (DC-4 Wing - DRIFTED 47 miles)', 'easting': 457500.5, 'northing': 4702750.2, 'depth_ft': 158, 'length_ft': 117, 'type': 'AIRCRAFT', 'shelf_lock': False},
    {'name': 'ZION-009 (DC-4 Wing - DRIFTED 47 miles)', 'easting': 457880.1, 'northing': 4702920.6, 'depth_ft': 139, 'length_ft': 115, 'type': 'AIRCRAFT', 'shelf_lock': False},
    {'name': 'ZION-010 (Large Vessel)', 'easting': 458550.4, 'northing': 4703280.9, 'depth_ft': 183, 'length_ft': 241, 'type': 'VESSEL', 'shelf_lock': False},
]

# DC-4 PRIMARY IMPACT SITE - 47 miles from Zion Trench
DC4_PRIMARY = {
    'name': 'DC-4 PRIMARY IMPACT SITE',
    'easting': 408500.0,
    'northing': 4760050.0,
    'depth_ft': 300,
    'distance_from_zion_miles': 47.1,
}

DC4_ENGINES = [
    {'name': 'DC-4 Engine 1', 'easting': 408560.3, 'northing': 4760010.5, 'depth_ft': 300},
    {'name': 'DC-4 Engine 2', 'easting': 408590.7, 'northing': 4760070.2, 'depth_ft': 300},
    {'name': 'DC-4 Engine 3', 'easting': 408470.5, 'northing': 4759980.8, 'depth_ft': 300},
    {'name': 'DC-4 Engine 4', 'easting': 408630.2, 'northing': 4760120.1, 'depth_ft': 300},
]


def generate_kml():
    # Build Zion Trench placemarks
    zion_placemarks = ''
    for t in ZION_TARGETS:
        lat, lon = utm_to_wgs84(t['easting'], t['northing'])
        if 'DC-4' in t['name']:
            color = 'ff00ffff'
            desc_extra = '<p><b>Note:</b> This wing fragment DRIFTED 47 miles from the primary impact site.</p>'
        elif t['type'] == 'ANCASTE':
            color = 'ff0000ff'
            desc_extra = '<p><b>PRIMARY ANCASTE CANDIDATE:</b> 180ft depth, 266ft length, 295deg heading</p>'
        else:
            color = 'ff00ffff'
            desc_extra = ''
        
        lock = 'ENGAGED' if t['shelf_lock'] else 'Not engaged'
        zion_placemarks += f'''
    <Placemark>
        <name>{t['name']}</name>
        <description><![CDATA[
            <h3>{t['name']}</h3>
            <p><b>Depth:</b> {t['depth_ft']} feet</p>
            <p><b>Length:</b> {t['length_ft']} feet</p>
            <p><b>Shelf-Lock:</b> {lock}</p>
            <p><b>Location:</b> Zion Trench, Lake Michigan</p>
            {desc_extra}
        ]]></description>
        <Style><IconStyle><color>{color}</color><scale>1.2</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/plt-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    # Build DC-4 Primary Impact placemarks
    lat, lon = utm_to_wgs84(DC4_PRIMARY['easting'], DC4_PRIMARY['northing'])
    dc4_primary_placemark = f'''
    <Placemark>
        <name>DC-4 PRIMARY IMPACT SITE (47 miles from Zion Trench)</name>
        <description><![CDATA[
            <h2>Flight 2501 Primary Impact</h2>
            <p><b>Date:</b> September 21, 1959</p>
            <p><b>Depth:</b> {DC4_PRIMARY['depth_ft']} feet</p>
            <p><b>Distance from Zion Trench:</b> {DC4_PRIMARY['distance_from_zion_miles']} miles WEST</p>
            <p><b>Contents:</b> Main fuselage, 4 engines, tail section, landing gear</p>
            <p><i>Heavy components sank immediately at impact site. Light debris (wings) drifted 47 miles east to Zion Trench.</i></p>
        ]]></description>
        <Style><IconStyle><color>ffff00ff</color><scale>1.5</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    dc4_engine_placemarks = ''
    for e in DC4_ENGINES:
        lat, lon = utm_to_wgs84(e['easting'], e['northing'])
        dc4_engine_placemarks += f'''
    <Placemark>
        <name>{e['name']} (Primary Impact Site)</name>
        <description><![CDATA[
            <h3>{e['name']}</h3>
            <p><b>Depth:</b> {e['depth_ft']} feet</p>
            <p><b>Location:</b> Primary impact site (47 miles from Zion Trench)</p>
            <p><i>Pratt & Whitney R-2000 radial engine - heavy, sank immediately</i></p>
        ]]></description>
        <Style><IconStyle><color>ff00ff00</color><scale>1.0</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    kml = f'''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
    <name>Flight 2501 Memorial - CORRECTED DISTANCES</name>
    <description><![CDATA[
        <h1>Flight 2501 Memorial</h1>
        <h2>In Memory of 58 Victims - September 21, 1959</h2>
        <hr/>
        <h2>Personal Dedication</h2>
        <p><b>In Memory of:</b> Denny Hadfield</p>
        <p><b>October 1944 - March 2025</b></p>
        <p><i>This search is dedicated to my father, Denny Hadfield, whose love for the Great Lakes and aviation inspired this lifelong quest. His spirit guides every sonar ping and satellite scan.</i></p>
        <hr/>
        <h2>IMPORTANT: Distance Clarification</h2>
        <p><b>DC-4 Primary Impact Site:</b> 47 miles WEST of Zion Trench (300ft depth)</p>
        <p><b>DC-4 Wing Fragments:</b> In Zion Trench (139-158ft depth) - DRIFTED 47 miles from impact</p>
        <p><b>SS Andaste Candidates:</b> In Zion Trench (180ft depth) - Separate wreck from 1907</p>
        <p><i>Light debris (wings) drifted with currents. Heavy debris (engines, fuselage) sank at impact site.</i></p>
        <hr/>
        <p><b>Generated:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
    ]]></description>
    
    <!-- Memorial -->
    <Placemark>
        <name>Flight 2501 Memorial - 58 Victims</name>
        <description><![CDATA[
            <h1>Northwest Orient Airlines Flight 2501</h1>
            <p><b>Date:</b> September 21, 1959</p>
            <p><b>Aircraft:</b> Douglas DC-4</p>
            <p><b>Victims:</b> 58 souls</p>
            <hr/>
            <p><b>Denny Hadfield</b><br/>October 1944 - March 2025</p>
        ]]></description>
        <Style><IconStyle><color>ffffffff</color><scale>1.5</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>-88.1224,42.9876,0</coordinates></Point>
    </Placemark>
    
    <!-- DC-4 Primary Impact Site -->
    <Folder>
        <name>DC-4 PRIMARY IMPACT SITE (47 miles from Zion Trench)</name>
        <description>Heavy debris that sank immediately at impact location. 47 miles WEST of Zion Trench.</description>
{dc4_primary_placemark}
{dc4_engine_placemarks}
    </Folder>
    
    <!-- Zion Trench - Andaste + Drifted DC-4 Wings -->
    <Folder>
        <name>Zion Trench - SS Andaste + DRIFTED DC-4 Wings</name>
        <description>SS Andaste candidates (1907 wreck) AND DC-4 wing fragments that DRIFTED 47 miles from primary impact. Light debris drifted east with currents.</description>
{zion_placemarks}
    </Folder>
    
    <!-- Distance Reference Line -->
    <Placemark>
        <name>47 Mile Distance - Impact to Zion Trench</name>
        <description>DC-4 wing fragments drifted 47 miles from primary impact (west) to Zion Trench (east)</description>
        <Style><LineStyle><color>ff0000ff</color><width>2</width></LineStyle></Style>
        <LineString>
            <coordinates>
                -88.1224,42.9876,0 -87.52,42.47,0
            </coordinates>
        </LineString>
    </Placemark>
</Document>
</kml>
'''
    return kml


def main():
    print("="*70)
    print("FLIGHT 2501 MEMORIAL KMZ - CORRECTED DISTANCES")
    print("="*70)
    print()
    print("In Memory of:")
    print("  Flight 2501 Victims - 58 souls (September 21, 1959)")
    print("  Denny Hadfield - October 1944 to March 2025")
    print()
    print("DISTANCE CLARIFICATION:")
    print("  DC-4 Primary Impact to Zion Trench: 47.1 miles")
    print("  DC-4 Wings (Zion) to Andaste (Zion): 0.3 miles")
    print()
    
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    kml = generate_kml()
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'flight2501_memorial_corrected_{timestamp}.kmz'
    
    with zipfile.ZipFile(kmz_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml)
    
    print(f"KMZ: {kmz_path}")
    print(f"Size: {kmz_path.stat().st_size / 1024:.1f} KB")
    print()
    print("="*70)
    print("🕊️ In Memory of Flight 2501 Victims")
    print("🕊️ In Loving Memory of Denny Hadfield (Oct 1944 - Mar 2025)")
    print("="*70)


if __name__ == '__main__':
    main()
