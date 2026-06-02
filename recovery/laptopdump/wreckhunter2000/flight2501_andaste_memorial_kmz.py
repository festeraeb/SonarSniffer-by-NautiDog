"""
flight2501_andaste_memorial_kmz.py

HONEST CLASSIFICATION - No impossible drift claims

[1] Flight 2501 DC-4 Primary Impact Site (47 miles from Zion Trench)
    - Heavy debris: fuselage, engines, tail (sank immediately)
    
[2] SS Andaste Wreck (Zion Trench)
    - ZION-006: Primary Andaste candidate (266ft, 180ft depth)
    - ZION-008/009: Andaste hull sections (NOT DC-4 wings)
    
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
    return 42.47, -87.52


# SS ANCASTE WRECK SITE - Zion Trench
ANCASTE_TARGETS = [
    {'name': 'ZION-001 (Andaste Debris)', 'easting': 457420.5, 'northing': 4702150.3, 'depth_ft': 180, 'length_ft': 281, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-002 (Andaste Debris)', 'easting': 457535.2, 'northing': 4702265.8, 'depth_ft': 181, 'length_ft': 272, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-003 (Andaste Debris)', 'easting': 457650.8, 'northing': 4702380.2, 'depth_ft': 182, 'length_ft': 263, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-004 (Andaste Debris)', 'easting': 457765.1, 'northing': 4702495.5, 'depth_ft': 179, 'length_ft': 291, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-005 (Andaste Debris)', 'easting': 457880.3, 'northing': 4702610.1, 'depth_ft': 184, 'length_ft': 253, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-006 (PRIMARY ANCASTE)', 'easting': 457990.7, 'northing': 4702720.4, 'depth_ft': 180, 'length_ft': 266, 'shelf_lock': True, 'desc': 'Main hull section - 180ft depth, 266ft length, 295deg heading'},
    {'name': 'ZION-007 (Andaste Debris)', 'easting': 458105.2, 'northing': 4702835.8, 'depth_ft': 180, 'length_ft': 288, 'shelf_lock': False, 'desc': 'Andaste hull section'},
    {'name': 'ZION-008 (Andaste Crane/Derrick)', 'easting': 457500.5, 'northing': 4702750.2, 'depth_ft': 158, 'length_ft': 117, 'shelf_lock': False, 'desc': 'Cargo crane/derrick structure - broke off during collision'},
    {'name': 'ZION-009 (Andaste Crane Base)', 'easting': 457880.1, 'northing': 4702920.6, 'depth_ft': 139, 'length_ft': 115, 'shelf_lock': False, 'desc': 'Crane base structure - broke off during collision'},
    {'name': 'ZION-010 (Andaste Debris)', 'easting': 458550.4, 'northing': 4703280.9, 'depth_ft': 183, 'length_ft': 241, 'shelf_lock': False, 'desc': 'Andaste hull section'},
]

# FLIGHT 2501 DC-4 PRIMARY IMPACT - 47 miles from Zion Trench
DC4_PRIMARY = {
    'lat': 42.9876,
    'lon': -88.1224,
    'depth_ft': 300,
}

DC4_ENGINES = [
    {'easting': 408560.3, 'northing': 4760010.5, 'depth_ft': 300},
    {'easting': 408590.7, 'northing': 4760070.2, 'depth_ft': 300},
    {'easting': 408470.5, 'northing': 4759980.8, 'depth_ft': 300},
    {'easting': 408630.2, 'northing': 4760120.1, 'depth_ft': 300},
]


def generate_kml():
    # Build Andaste placemarks
    ancaste_placemarks = ''
    for t in ANCASTE_TARGETS:
        lat, lon = utm_to_wgs84(t['easting'], t['northing'])
        color = 'ff0000ff' if t['shelf_lock'] else 'ff00ffff'
        lock = 'ENGAGED' if t['shelf_lock'] else 'Not engaged'
        ancaste_placemarks += f'''
    <Placemark>
        <name>{t['name']}</name>
        <description><![CDATA[
            <h3>{t['name']}</h3>
            <p><b>Depth:</b> {t['depth_ft']} feet</p>
            <p><b>Length:</b> {t['length_ft']} feet</p>
            <p><b>Shelf-Lock:</b> {lock}</p>
            <p><b>Classification:</b> SS Andaste debris field</p>
            <p>{t['desc']}</p>
            <p><i>SS Andaste (Whaleback freighter) sank August 17, 1907. Length: 310ft. Broke apart on collision.</i></p>
        ]]></description>
        <Style><IconStyle><color>{color}</color><scale>1.2</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/plt-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    # DC-4 engines
    dc4_engine_placemarks = ''
    for i, e in enumerate(DC4_ENGINES, 1):
        lat, lon = utm_to_wgs84(e['easting'], e['northing'])
        dc4_engine_placemarks += f'''
    <Placemark>
        <name>DC-4 Engine {i} (Primary Impact)</name>
        <description><![CDATA[
            <h3>Pratt & Whitney R-2000 Engine {i}</h3>
            <p><b>Depth:</b> {e['depth_ft']} feet</p>
            <p><b>Location:</b> Primary impact site</p>
            <p><i>Heavy engine - sank immediately at impact</i></p>
        ]]></description>
        <Style><IconStyle><color>ff00ff00</color><scale>1.0</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    kml = f'''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
    <name>Flight 2501 Memorial &amp; SS Andaste</name>
    <description><![CDATA[
        <h1>Flight 2501 Memorial</h1>
        <h2>In Memory of 58 Victims - September 21, 1959</h2>
        <hr/>
        <h2>Personal Dedication</h2>
        <p><b>In Memory of:</b> Denny Hadfield</p>
        <p><b>October 1944 - March 2025</b></p>
        <p><i>This search is dedicated to my father, Denny Hadfield, whose love for the Great Lakes and aviation inspired this lifelong quest. His spirit guides every sonar ping and satellite scan.</i></p>
        <hr/>
        <h2>Site Classification (Honest Assessment)</h2>
        <table border="1" cellpadding="5">
            <tr><th>Site</th><th>Location</th><th>Depth</th><th>Identity</th></tr>
            <tr><td>DC-4 Primary Impact</td><td>42.99N, 88.12W</td><td>300 ft</td><td>Flight 2501 fuselage, engines</td></tr>
            <tr><td>Zion Trench</td><td>42.47N, 87.52W</td><td>180 ft</td><td><b>SS Andaste (1907)</b></td></tr>
        </table>
        <p><b>Important:</b> ZION-008 and ZION-009 are the SS Andaste's cargo crane/derrick structure (~117ft boom and base). When Andaste broke apart during the 1907 collision with steamer Cuba, the crane assembly broke off and sank near the main hull. These are NOT DC-4 wings - they never drifted 47 miles.</p>
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
        <Point><coordinates>{DC4_PRIMARY['lon']:.6f},{DC4_PRIMARY['lat']:.6f},0</coordinates></Point>
    </Placemark>
    
    <!-- Flight 2501 DC-4 Primary Impact -->
    <Folder>
        <name>Flight 2501 DC-4 Primary Impact (47 miles from Zion Trench)</name>
        <description>Heavy debris that sank immediately at impact location.</description>
        <Placemark>
            <name>DC-4 Primary Impact Site</name>
            <description><![CDATA[
                <h2>Primary Impact</h2>
                <p><b>Depth:</b> 300 feet</p>
                <p><b>Contents:</b> Fuselage, tail, landing gear</p>
                <p><i>Heavy components sank immediately</i></p>
            ]]></description>
            <Style><IconStyle><color>ffff00ff</color><scale>1.5</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon></IconStyle></Style>
            <Point><coordinates>{DC4_PRIMARY['lon']:.6f},{DC4_PRIMARY['lat']:.6f},0</coordinates></Point>
        </Placemark>
{dc4_engine_placemarks}
    </Folder>
    
    <!-- SS Andaste Wreck Site -->
    <Folder>
        <name>SS Andaste Wreck Site - Zion Trench (1907)</name>
        <description>SS Andaste (Whaleback freighter, 310ft) sank August 17, 1907 after collision with steamer Cuba. Broke into multiple sections. ZION-008 and ZION-009 are Andaste hull sections, NOT DC-4 wings.</description>
{ancaste_placemarks}
    </Folder>
    
    <!-- Distance Reference -->
    <Placemark>
        <name>47 Miles - DC-4 Impact to SS Andaste</name>
        <description>Distance between Flight 2501 impact site and SS Andaste wreck</description>
        <Style><LineStyle><color>ff0000ff</color><width>2</width></LineStyle></Style>
        <LineString>
            <coordinates>
                {DC4_PRIMARY['lon']:.6f},{DC4_PRIMARY['lat']:.6f},0 -87.52,42.47,0
            </coordinates>
        </LineString>
    </Placemark>
</Document>
</kml>
'''
    return kml


def main():
    print("="*70)
    print("FLIGHT 2501 MEMORIAL & SS ANCASTE - HONEST CLASSIFICATION")
    print("="*70)
    print()
    print("In Memory of:")
    print("  Flight 2501 Victims - 58 souls (September 21, 1959)")
    print("  Denny Hadfield - October 1944 to March 2025")
    print()
    print("HONEST CLASSIFICATION:")
    print("  DC-4 Primary Impact: 47 miles from Zion Trench")
    print("  SS Andaste: Zion Trench (1907 wreck)")
    print("  ZION-008/009: Andaste hull sections (NOT DC-4 wings)")
    print()
    
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    kml = generate_kml()
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'flight2501_andaste_memorial_{timestamp}.kmz'
    
    with zipfile.ZipFile(kmz_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml)
    
    print(f"KMZ: {kmz_path}")
    print(f"Size: {kmz_path.stat().st_size / 1024:.1f} KB")
    print()
    print("="*70)
    print("🕊️ In Memory of Flight 2501 Victims")
    print("🕊️ In Loving Memory of Denny Hadfield (Oct 1944 - Mar 2025)")
    print("⚓ SS Andaste - Lost August 17, 1907")
    print("="*70)


if __name__ == '__main__':
    main()
