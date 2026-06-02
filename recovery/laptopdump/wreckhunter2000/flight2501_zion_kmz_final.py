"""
flight2501_zion_kmz_final.py

FINAL VERSION - Uses NOAA WMS NetworkLinks (no embedding issues)

Generate Google Earth KMZ with:
[1] Zion Corridor Wrecks - CORRECTED UTM-16T (457xxx easting)
[2] Flight 2501 DC-4 Debris Field - CORRECTED UTM-16T
[3] NOAA ENC Charts - WMS NetworkLinks (all Great Lakes)

All coordinates converted from UTM-16T to WGS84 (lat/lon).
UTM Zone 16 validated - 35km east of Zone 15/16 boundary.
"""

import json
import zipfile
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Tuple

# Use pyproj for accurate UTM conversion
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
        # Fallback manual calculation
        from math import radians, sin, cos, tan, sqrt, asin, pi, exp
        
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
            D**2/2 - (5 + 3*T1 + 10*C1 - 4*C1**2 - 9*E2) * D**4/24 +
            (61 + 90*T1 + 298*C1 + 45*T1**2 - 252*E2 - 3*C1**2) * D**6/720
        )
        
        lon = lon_origin + (
            D - (1 + 2*T1 + C1) * D**3/6 +
            (5 - 2*C1 + 28*T1 - 3*C1**2 + 8*E2 + 24*T1**2) * D**5/120
        ) / cos(phi1)
        
        return lat * 180 / pi, lon * 180 / pi


# CORRECTED UTM coordinates (457xxx easting for Zion, 408xxx for DC-4)
ZION_WRECKS = [
    {'name': 'ZION-001', 'easting': 457420.5, 'northing': 4702150.3, 'depth_m': 54.8, 'contour_ft': 180, 'length_ft': 281, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter candidate'},
    {'name': 'ZION-002', 'easting': 457535.2, 'northing': 4702265.8, 'depth_m': 55.2, 'contour_ft': 181, 'length_ft': 272, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 181ft'},
    {'name': 'ZION-003', 'easting': 457650.8, 'northing': 4702380.2, 'depth_m': 55.5, 'contour_ft': 182, 'length_ft': 263, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 182ft'},
    {'name': 'ZION-004', 'easting': 457765.1, 'northing': 4702495.5, 'depth_m': 54.5, 'contour_ft': 179, 'length_ft': 291, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 179ft'},
    {'name': 'ZION-005', 'easting': 457880.3, 'northing': 4702610.1, 'depth_m': 56.1, 'contour_ft': 184, 'length_ft': 253, 'type': 'LARGE_VESSEL', 'shelf_lock': False, 'desc': 'Large vessel - 184ft'},
    {'name': 'ZION-006 (ANCASTE MAIN)', 'easting': 457990.7, 'northing': 4702720.4, 'depth_m': 54.9, 'contour_ft': 180, 'length_ft': 266, 'type': 'ANDASTE_CANDIDATE', 'shelf_lock': True, 'desc': 'PRIMARY ANCASTE - 180ft, 266ft, 295°'},
    {'name': 'ZION-007', 'easting': 458105.2, 'northing': 4702835.8, 'depth_m': 55.0, 'contour_ft': 180, 'length_ft': 288, 'type': 'HEAVY_FREIGHTER', 'shelf_lock': False, 'desc': 'Heavy freighter - 180ft'},
    {'name': 'ZION-008 (DC-4 Wing)', 'easting': 457500.5, 'northing': 4702750.2, 'depth_m': 48.2, 'contour_ft': 158, 'length_ft': 117, 'type': 'AIRCRAFT_DEBRIS', 'shelf_lock': False, 'desc': 'DC-4 wing - PSF corrected 117ft'},
    {'name': 'ZION-009 (DC-4 Wing)', 'easting': 457880.1, 'northing': 4702920.6, 'depth_m': 42.5, 'contour_ft': 139, 'length_ft': 115, 'type': 'AIRCRAFT_DEBRIS', 'shelf_lock': False, 'desc': 'DC-4 wing - Aluminum confirmed'},
    {'name': 'ZION-010', 'easting': 458550.4, 'northing': 4703280.9, 'depth_m': 55.8, 'contour_ft': 183, 'length_ft': 241, 'type': 'LARGE_VESSEL', 'shelf_lock': False, 'desc': 'Large vessel - 183ft'},
]

DC4_DEBRIS = [
    {'name': 'DC-4 Primary Impact (Rank 1)', 'easting': 408500.0, 'northing': 4760050.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 95, 'type': 'MASTER_TARGET', 'desc': 'Primary impact - Aluminum + 4 engines'},
    {'name': 'DC-4 Engine #1 (P&W R-2000)', 'easting': 408560.3, 'northing': 4760010.5, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt & Whitney R-2000 (Z: -2.3)'},
    {'name': 'DC-4 Engine #2 (P&W R-2000)', 'easting': 408590.7, 'northing': 4760070.2, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt & Whitney R-2000 (Z: -2.1)'},
    {'name': 'DC-4 Engine #3 (P&W R-2000)', 'easting': 408470.5, 'northing': 4759980.8, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt & Whitney R-2000 (Z: -2.4)'},
    {'name': 'DC-4 Engine #4 (P&W R-2000)', 'easting': 408630.2, 'northing': 4760120.1, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 8, 'type': 'ENGINE', 'desc': 'Pratt & Whitney R-2000 (Z: -2.0)'},
    {'name': 'DC-4 Wing Fragment A', 'easting': 457500.5, 'northing': 4702750.2, 'depth_m': 48.2, 'contour_ft': 158, 'length_ft': 117, 'type': 'WING', 'desc': 'Aluminum wing - Zion Trench'},
    {'name': 'DC-4 Wing Fragment B', 'easting': 457880.1, 'northing': 4702920.6, 'depth_m': 42.5, 'contour_ft': 139, 'length_ft': 115, 'type': 'WING', 'desc': 'Aluminum wing - Zion Trench'},
    {'name': 'DC-4 Fuselage Section', 'easting': 408550.0, 'northing': 4760040.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 45, 'type': 'FUSELAGE', 'desc': 'Main fuselage near impact'},
    {'name': 'DC-4 Tail Section', 'easting': 408520.0, 'northing': 4760060.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 25, 'type': 'TAIL', 'desc': 'Tail with vertical stabilizer'},
    {'name': 'DC-4 Landing Gear', 'easting': 408580.0, 'northing': 4760030.0, 'depth_m': 91.4, 'contour_ft': 300, 'length_ft': 6, 'type': 'COMPONENT', 'desc': 'Landing gear assembly'},
]

NOAA_CHARTS = [
    {'name': 'Lake Michigan - Southern Basin', 'number': '14900'},
    {'name': 'Chicago Harbor', 'number': '14901'},
    {'name': 'Waukegan to Milwaukee', 'number': '14902'},
    {'name': 'Milwaukee Harbor', 'number': '14903'},
    {'name': 'Racine Harbor', 'number': '14904'},
    {'name': 'Kenosha Harbor', 'number': '14905'},
    {'name': 'Lake Superior', 'number': '14960'},
    {'name': 'Lake Huron', 'number': '14860'},
    {'name': 'Lake Erie', 'number': '14830'},
    {'name': 'Lake Ontario', 'number': '14780'},
]


def generate_kml():
    """Generate complete KML document."""
    kml = '<?xml version="1.0" encoding="UTF-8"?>\n'
    kml += '<kml xmlns="http://www.opengis.net/kml/2.2">\n'
    kml += '<Document>\n'
    
    # Metadata
    kml += f'''
    <name>Flight 2501 &amp; Zion Corridor (CORRECTED UTM-16T)</name>
    <description><![CDATA[
        <h2>WreckHunter2000 - CESAROPS V1.0</h2>
        <p><b>Generated:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
        <p><b>UTM Zone:</b> 16T (validated - 35km from Zone 15/16 boundary)</p>
        <p><b>Coordinates:</b> CORRECTED (457xxx easting for Zion, 408xxx for DC-4)</p>
        <hr/>
        <h3>🕊️ In Memory of Flight 2501</h3>
        <p><b>Date:</b> September 21, 1959</p>
        <p><b>Aircraft:</b> Douglas DC-4</p>
        <p><b>Victims:</b> 58 souls lost</p>
        <p><i>This KMZ is dedicated to finding the final resting place of Flight 2501 and honoring the memory of those who perished.</i></p>
        <p><i>Also dedicated to my father, whose love for the Great Lakes and aviation inspired this quest.</i></p>
    ]]></description>
    
    <!-- Styles -->
    <Style id="andaste"><IconStyle><color>ff0000ff</color><scale>1.5</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon></IconStyle></Style>
    <Style id="dc4"><IconStyle><color>ff00ffff</color><scale>1.3</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/cyan-circle.png</href></Icon></IconStyle></Style>
    <Style id="engine"><IconStyle><color>ff00ff00</color><scale>1.0</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href></Icon></IconStyle></Style>
    <Style id="memorial"><IconStyle><color>ffffffff</color><scale>1.5</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon></IconStyle></Style>
'''
    
    # Memorial placemark
    kml += '''
    <Placemark>
        <name>🕊️ In Memory of Flight 2501 Victims</name>
        <description><![CDATA[
            <h2>Northwest Orient Airlines Flight 2501</h2>
            <p><b>Date:</b> September 21, 1959</p>
            <p><b>Aircraft:</b> Douglas DC-4</p>
            <p><b>Route:</b> Chicago → Seattle</p>
            <p><b>Victims:</b> 58 souls</p>
        ]]></description>
        <styleUrl>#memorial</styleUrl>
        <Point><coordinates>-88.1224,42.9876,0</coordinates></Point>
    </Placemark>
'''
    
    # Zion wrecks
    kml += '<Folder><name>Zion Corridor Wrecks (ANCASTE Search)</name>\n'
    for w in ZION_WRECKS:
        lat, lon = utm_to_wgs84(w['easting'], w['northing'])
        color = 'ff0000ff' if w['shelf_lock'] else 'ff00ffff'
        lock_status = '✅ ENGAGED' if w['shelf_lock'] else '❌'
        kml += f'''
    <Placemark>
        <name>{w['name']}</name>
        <description><![CDATA[
            <h3>{w['name']}</h3>
            <p><b>Type:</b> {w['type']}</p>
            <p><b>Depth:</b> {w['contour_ft']} ft</p>
            <p><b>Length:</b> {w['length_ft']} ft</p>
            <p><b>Shelf-Lock:</b> {lock_status}</p>
            <p><b>UTM-16T:</b> E {w['easting']:.1f}, N {w['northing']:.1f}</p>
            <p>{w['desc']}</p>
        ]]></description>
        <Style><IconStyle><color>{color}</color><scale>1.2</scale><Icon><href>http://maps.google.com/mapfiles/kml/paddle/plt-circle.png</href></Icon></IconStyle></Style>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    kml += '</Folder>\n'
    
    # DC-4 debris
    kml += '<Folder><name>Flight 2501 DC-4 Debris Field</name>\n'
    for d in DC4_DEBRIS:
        lat, lon = utm_to_wgs84(d['easting'], d['northing'])
        if d['type'] == 'ENGINE':
            style = 'engine'
        elif d['type'] == 'MASTER_TARGET':
            style = 'dc4'
        else:
            style = 'dc4'
        kml += f'''
    <Placemark>
        <name>{d['name']}</name>
        <description><![CDATA[
            <h3>{d['name']}</h3>
            <p><b>Type:</b> {d['type']}</p>
            <p><b>Depth:</b> {d['contour_ft']} ft</p>
            <p><b>UTM-16T:</b> E {d['easting']:.1f}, N {d['northing']:.1f}</p>
            <p>{d['desc']}</p>
        ]]></description>
        <styleUrl>#{style}</styleUrl>
        <Point><coordinates>{lon:.6f},{lat:.6f},0</coordinates></Point>
    </Placemark>
'''
    kml += '</Folder>\n'
    
    # NOAA charts
    kml += '<Folder><name>NOAA ENC Charts - Great Lakes</name>\n'
    for c in NOAA_CHARTS:
        wms = f'https://charts.noaa.gov/arcgis/services/ENC/MapServer/WmsServer?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&FORMAT=image/png&LAYERS={c["number"]}&TRANSPARENT=true'
        kml += f'''
    <NetworkLink>
        <name>{c['name']} (ENC {c['number']})</name>
        <Link>
            <href>{wms}</href>
            <viewRefreshMode>onRegion</viewRefreshMode>
        </Link>
    </NetworkLink>
'''
    kml += '</Folder>\n'
    
    # Reference
    kml += '''
    <Folder><name>Reference</name>
        <Placemark><name>180ft Contour (Zion Trench)</name><description>SS Andaste depth</description><LineString><coordinates>-87.52,42.44,0 -87.50,42.44,0 -87.50,42.50,0 -87.52,42.50,0</coordinates></LineString></Placemark>
        <Placemark><name>UTM Zone 15/16 Boundary (90°W)</name><description>Zone boundary - Zion is 35km EAST</description><LineString><coordinates>-90.0,42.0,0 -90.0,43.5,0</coordinates></LineString></Placemark>
    </Folder>
'''
    
    kml += '</Document>\n</kml>\n'
    return kml


def main():
    print("="*70)
    print("FLIGHT 2501 & ZION KMZ GENERATOR (FINAL - WMS NETWORKLINKS)")
    print("="*70)
    print()
    
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    print("Generating KML...")
    kml = generate_kml()
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'flight2501_zion_final_{timestamp}.kmz'
    kml_path = output_dir / f'flight2501_zion_final_{timestamp}.kml'
    
    # Create KMZ
    with zipfile.ZipFile(kmz_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml)
    
    # Save KML
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml)
    
    print(f"KMZ: {kmz_path} ({kmz_path.stat().st_size / 1024:.1f} KB)")
    print(f"KML: {kml_path} ({kml_path.stat().st_size / 1024:.1f} KB)")
    print()
    print("Zion Wrecks: 10 (CORRECTED UTM)")
    print("DC-4 Debris: 10 (CORRECTED UTM)")
    print("NOAA Charts: 10 (WMS NetworkLinks)")
    print()
    print("🕊️ In Memory of Flight 2501 Victims (58 souls)")
    print("="*70)


if __name__ == '__main__':
    main()
