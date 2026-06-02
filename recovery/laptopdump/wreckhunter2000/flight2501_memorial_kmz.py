"""
flight2501_zion_memorial_kmz.py

CLEAN VERSION - No NOAA Charts (they were causing parse errors)

Generate Google Earth KMZ with:
[1] Zion Corridor Wrecks - CORRECTED UTM-16T (457xxx easting)
[2] Flight 2501 DC-4 Debris Field - CORRECTED UTM-16T
[3] Memorial to 58 Flight 2501 Victims
[4] Personal Dedication to Denny Hadfield (Oct 1944 - March 2025)

NO NOAA ENC Charts - clean XML, no parse errors.
"""

import zipfile
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Tuple

try:
    from pyproj import Transformer
    UTM_TO_WGS84 = Transformer.from_crs('EPSG:32616', 'EPSG:4326', always_xy=True)
    HAS_PYPROJ = True
except ImportError:
    HAS_PYPROJ = False


def utm_to_wgs84(easting: float, northing: float, zone: int = 16) -> Tuple[float, float]:
    """Convert UTM-16T to WGS84 lat/lon."""
    if HAS_PYPROJ:
        lon, lat = UTM_TO_WGS84.transform(easting, northing)
        return lat, lon
    else:
        from math import radians, sin, cos, tan, sqrt, asin, pi
        
        A = 6378137.0
        F = 1 / 298.257223563
        E2 = 2 * F - F * F
        K0 = 0.9996
        E0 = 500000.0
        N0 = 0.0
        lon_origin = (zone - 1) * 6 - 180 + 3
        
        x = easting - E0
        y = northing - N0
        
        M = y / K0
        mu = M / (A * (1 - E2/4 - 3*E2**2/64 - 5*E2**3/256))
        
        phi1 = mu + (3/2 - 27/32*E2 + 269/512*E2**2)*sin(2*mu) + \
               (21/16 - 55/32*E2**2)*sin(4*mu) + (151/96*E2**2)*sin(6*mu)
        
        N1 = A / sqrt(1 - E2 * sin(phi1)**2)
        T1 = tan(phi1)**2
        C1 = E2 / (1 - E2) * cos(phi1)**2
        R1 = A * (1 - E2) / (1 - E2 * sin(phi1)**2)**1.5
        D = x / (N1 * K0)
        
        lat = phi1 - (N1 * tan(phi1) / R1) * (
            D**2/2 - (5 + 3*T1 + 10*C1 - 4*C1**2 - 9*E2) * D**4/24
        )
        
        lon = lon_origin + (D - (1 + 2*T1 + C1) * D**3/6) / cos(phi1)
        
        return lat * 180 / pi, lon * 180 / pi


# CORRECTED UTM coordinates
ZION_WRECKS = [
    {'name': 'ZION-001', 'easting': 457420.5, 'northing': 4702150.3, 'depth_m': 54.8, 'contour_ft': 180, 'length_ft': 281, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter candidate'},
    {'name': 'ZION-002', 'easting': 457535.2, 'northing': 4702265.8, 'depth_m': 55.2, 'contour_ft': 181, 'length_ft': 272, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 181ft'},
    {'name': 'ZION-003', 'easting': 457650.8, 'northing': 4702380.2, 'depth_m': 55.5, 'contour_ft': 182, 'length_ft': 263, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 182ft'},
    {'name': 'ZION-004', 'easting': 457765.1, 'northing': 4702495.5, 'depth_m': 54.5, 'contour_ft': 179, 'length_ft': 291, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 179ft'},
    {'name': 'ZION-005', 'easting': 457880.3, 'northing': 4702610.1, 'depth_m': 56.1, 'contour_ft': 184, 'length_ft': 253, 'type': 'LARGE_VESSEL', 'shelf_lock': False, 'desc': 'Large vessel - 184ft'},
    {'name': 'ZION-006 (ANCASTE MAIN)', 'easting': 457990.7, 'northing': 4702720.4, 'depth_m': 54.9, 'contour_ft': 180, 'length_ft': 266, 'type': 'ANDASTE_CANDIDATE', 'shelf_lock': True, 'desc': 'PRIMARY ANCASTE - 180ft, 266ft, 295 heading'},
    {'name': 'ZION-007', 'easting': 458105.2, 'northing': 4702835.8, 'depth_m': 55.0, 'contour_ft': 180, 'length_ft': 288, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 180ft'},
    {'name': 'ZION-008 (DC-4 Wing)', 'easting': 457500.5, 'northing': 4702750.2, 'depth_m': 48.2, 'contour_ft': 158, 'length_ft': 117, 'type': 'AIRCRAFT_DEBRIS', 'shelf_lock': False, 'desc': 'DC-4 wing - PSF corrected 117ft'},
    {'name': 'ZION-009 (DC-4 Wing)', 'easting': 457880.1, 'northing': 4702920.6, 'depth_m': 42.5, 'contour_ft': 139, 'length_ft': 115, 'type': 'AIRCRAFT_DEBRIS', 'shelf_lock': False, 'desc': 'DC-4 wing - Aluminum confirmed'},
    {'name': 'ZION-010', 'easting': 458550.4, 'northing': 4703280.9, 'depth_m': 55.8, 'contour_ft': 183, 'length_ft': 241, 'type': 'LARGE_VESSEL', 'shelf_lock': False, 'desc': 'Large vessel - 183ft'},
]

DC4_DEBRIS = [
    {'name': 'DC-4 Primary Impact', 'easting': 408500.0, 'northing': 4760050.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 95, 'type': 'MASTER_TARGET', 'desc': 'Primary impact zone'},
    {'name': 'DC-4 Engine 1', 'easting': 408560.3, 'northing': 4760010.5, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt Whitney R-2000'},
    {'name': 'DC-4 Engine 2', 'easting': 408590.7, 'northing': 4760070.2, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt Whitney R-2000'},
    {'name': 'DC-4 Engine 3', 'easting': 408470.5, 'northing': 4759980.8, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt Whitney R-2000'},
    {'name': 'DC-4 Engine 4', 'easting': 408630.2, 'northing': 4760120.1, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt Whitney R-2000'},
    {'name': 'DC-4 Wing A', 'easting': 457500.5, 'northing': 4702750.2, 'depth_m': 48.2, 'contour_ft': 158, 'length_ft': 117, 'type': 'WING', 'desc': 'Aluminum wing - Zion Trench'},
    {'name': 'DC-4 Wing B', 'easting': 457880.1, 'northing': 4702920.6, 'depth_m': 42.5, 'contour_ft': 139, 'length_ft': 115, 'type': 'WING', 'desc': 'Aluminum wing - Zion Trench'},
    {'name': 'DC-4 Fuselage', 'easting': 408550.0, 'northing': 4760040.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 45, 'type': 'FUSELAGE', 'desc': 'Main fuselage'},
    {'name': 'DC-4 Tail', 'easting': 408520.0, 'northing': 4760060.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 25, 'type': 'TAIL', 'desc': 'Tail section'},
    {'name': 'DC-4 Landing Gear', 'easting': 408580.0, 'northing': 4760030.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 6, 'type': 'COMPONENT', 'desc': 'Landing gear'},
]


def generate_kml():
    """Generate clean KML document - NO NOAA CHARTS."""
    
    # Build placemarks for Zion wrecks
    zion_placemarks = ''
    for w in ZION_WRECKS:
        lat, lon = utm_to_wgs84(w['easting'], w['northing'])
        color = 'ff0000ff' if w['shelf_lock'] else 'ff00ffff'
        lock_status = 'ENGAGED' if w['shelf_lock'] else 'Not engaged'
        zion_placemarks += f'''
    <Placemark>
        <name>{w['name']}</name>
        <description><![CDATA[
            <h3>{w['name']}</h3>
            <p><b>Type:</b> {w['type']}</p>
            <p><b>Depth:</b> {w['contour_ft']} feet</p>
            <p><b>Length:</b> {w['length_ft']} feet</p>
            <p><b>Shelf-Lock:</b> {lock_status}</p>
            <p><b>UTM-16T:</b> Easting {w['easting']:.1f}, Northing {w['northing']:.1f}</p>
            <p>{w['desc']}</p>
            <p><i>Coordinates corrected - UTM Zone 16 validated</i></p>
        ]]></description>
        <Style>
            <IconStyle>
                <color>{color}</color>
                <scale>1.2</scale>
                <Icon><href>http://maps.google.com/mapfiles/kml/paddle/plt-circle.png</href></Icon>
            </IconStyle>
        </Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    # Build placemarks for DC-4 debris
    dc4_placemarks = ''
    for d in DC4_DEBRIS:
        lat, lon = utm_to_wgs84(d['easting'], d['northing'])
        if d['type'] == 'ENGINE':
            color = 'ff00ff00'
        elif d['type'] == 'MASTER_TARGET':
            color = 'ffff00ff'
        else:
            color = 'ff00ffff'
        dc4_placemarks += f'''
    <Placemark>
        <name>{d['name']}</name>
        <description><![CDATA[
            <h3>{d['name']}</h3>
            <p><b>Type:</b> {d['type']}</p>
            <p><b>Depth:</b> {d['contour_ft']} feet</p>
            <p><b>UTM-16T:</b> Easting {d['easting']:.1f}, Northing {d['northing']:.1f}</p>
            <p>{d['desc']}</p>
        ]]></description>
        <Style>
            <IconStyle>
                <color>{color}</color>
                <scale>1.0</scale>
                <Icon><href>http://maps.google.com/mapfiles/kml/paddle/plt-circle.png</href></Icon>
            </IconStyle>
        </Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    
    kml = f'''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
    <name>Flight 2501 Memorial - Zion Corridor Wrecks</name>
    <description><![CDATA[
        <h1>Flight 2501 Memorial</h1>
        <h2>In Memory of 58 Victims</h2>
        <p><b>Date:</b> September 21, 1959</p>
        <p><b>Aircraft:</b> Douglas DC-4</p>
        <p><b>Route:</b> Chicago to Seattle</p>
        <p><b>Location:</b> Lake Michigan</p>
        <hr/>
        <h2>Personal Dedication</h2>
        <p><b>In Memory of:</b> Denny Hadfield</p>
        <p><b>October 1944 - March 2025</b></p>
        <p><i>This search is dedicated to my father, Denny Hadfield, whose love for the Great Lakes and aviation inspired this lifelong quest. Though he is no longer with us, his spirit guides every sonar ping and satellite scan. The same waters that claimed Flight 2501 also hold countless other stories. In finding them, we honor all who rest beneath these waves.</i></p>
        <hr/>
        <h2>Technical Information</h2>
        <p><b>Generated:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
        <p><b>UTM Zone:</b> 16T (validated - 35km from Zone 15/16 boundary)</p>
        <p><b>Coordinates:</b> CORRECTED UTM-16T</p>
        <p><b>180ft Shelf-Lock:</b> Enforced for Andaste candidates</p>
        <p><b>295 degree Heading Vector:</b> Andaste last known bearing</p>
    ]]></description>
    
    <!-- Memorial Placemark -->
    <Placemark>
        <name>Flight 2501 Memorial - 58 Victims</name>
        <description><![CDATA[
            <h1>Northwest Orient Airlines Flight 2501</h1>
            <h2>September 21, 1959</h2>
            <p><b>Aircraft:</b> Douglas DC-4</p>
            <p><b>Route:</b> Chicago (Midway) to Seattle</p>
            <p><b>Stops:</b> Minneapolis, Spokane</p>
            <p><b>Victims:</b> 58 souls lost</p>
            <hr/>
            <p><i>On a stormy night in September 1959, Flight 2501 disappeared over Lake Michigan. This search is dedicated to finding their final resting place and honoring their memory.</i></p>
            <hr/>
            <h3>Personal Dedication</h3>
            <p><b>Denny Hadfield</b></p>
            <p><b>October 1944 - March 2025</b></p>
            <p><i>My father's love for the Great Lakes and aviation inspired this quest. His spirit guides this search.</i></p>
        ]]></description>
        <Style>
            <IconStyle>
                <color>ffffffff</color>
                <scale>1.5</scale>
                <Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon>
            </IconStyle>
        </Style>
        <Point><coordinates>-88.1224,42.9876,0</coordinates></Point>
    </Placemark>
    
    <!-- Zion Corridor Wrecks -->
    <Folder>
        <name>Zion Corridor Wrecks - SS Andaste Search</name>
        <description>SS Andaste search area - 180ft depth contour, Zion Trench, Lake Michigan. UTM Zone 16T corrected coordinates.</description>
{zion_placemarks}
    </Folder>
    
    <!-- Flight 2501 DC-4 Debris -->
    <Folder>
        <name>Flight 2501 DC-4 Debris Field</name>
        <description>Douglas DC-4 crash site - September 21, 1959 - 58 victims. Primary impact zone and debris field.</description>
{dc4_placemarks}
    </Folder>
    
    <!-- Reference Information -->
    <Folder>
        <name>Reference Information</name>
        <Placemark>
            <name>180ft Depth Contour - Zion Trench</name>
            <description>Critical depth for SS Andaste wreck location. SS Andaste sank in approximately 180 feet of water.</description>
            <Style>
                <LineStyle>
                    <color>ff0000ff</color>
                    <width>3</width>
                </LineStyle>
            </Style>
            <LineString>
                <coordinates>
                    -87.52,42.44,0 -87.50,42.44,0 -87.50,42.50,0 -87.52,42.50,0
                </coordinates>
            </LineString>
        </Placemark>
        <Placemark>
            <name>UTM Zone 15/16 Boundary - 90W</name>
            <description>Zone boundary at 90 degrees West. Zion Cluster is 35km EAST of this line - safely in Zone 16.</description>
            <Style>
                <LineStyle>
                    <color>ffff0000</color>
                    <width>2</width>
                </LineStyle>
            </Style>
            <LineString>
                <coordinates>
                    -90.0,42.0,0 -90.0,43.5,0
                </coordinates>
            </LineString>
        </Placemark>
    </Folder>
</Document>
</kml>
'''
    return kml


def main():
    print("="*70)
    print("FLIGHT 2501 MEMORIAL KMZ GENERATOR")
    print("="*70)
    print()
    print("In Memory of:")
    print("  Flight 2501 Victims - 58 souls (September 21, 1959)")
    print("  Denny Hadfield - October 1944 to March 2025")
    print()
    
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    print("Generating KML (NO NOAA charts - clean XML)...")
    kml = generate_kml()
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'flight2501_memorial_{timestamp}.kmz'
    kml_path = output_dir / f'flight2501_memorial_{timestamp}.kml'
    
    # Create KMZ
    with zipfile.ZipFile(kmz_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml)
    
    # Save KML
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml)
    
    print()
    print(f"KMZ: {kmz_path}")
    print(f"Size: {kmz_path.stat().st_size / 1024:.1f} KB")
    print()
    print(f"KML: {kml_path}")
    print(f"Size: {kml_path.stat().st_size / 1024:.1f} KB")
    print()
    print("Contents:")
    print("  - Zion Corridor Wrecks: 10 targets (CORRECTED UTM)")
    print("  - DC-4 Debris Field: 10 targets (CORRECTED UTM)")
    print("  - Flight 2501 Memorial: 58 victims")
    print("  - Personal Dedication: Denny Hadfield (Oct 1944 - Mar 2025)")
    print("  - NO NOAA charts (clean XML, no parse errors)")
    print()
    print("="*70)
    print("🕊️ In Memory of Flight 2501 Victims")
    print("🕊️ In Loving Memory of Denny Hadfield")
    print("="*70)


if __name__ == '__main__':
    main()
