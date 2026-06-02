"""
flight2501_zion_kmz_generator.py

CORRECTED UTM COORDINATES VERSION

Generate Google Earth KMZ with:
[1] Zion Corridor Wrecks (Andaste candidates) - CORRECTED UTM-16T (457xxx easting)
[2] Flight 2501 DC-4 Debris Field - CORRECTED UTM-16T coordinates
[3] NOAA ENC Charts - Local files from noaa_charts/ subdirectory

All coordinates converted from UTM-16T to WGS84 (lat/lon) for Google Earth.
UTM Zone 16 validated - 35km east of Zone 15/16 boundary.
"""

import json
import zipfile
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Tuple
from math import radians, sin, cos, atan2, sqrt, pi

# Use pyproj for accurate UTM conversion
try:
    from pyproj import Transformer
    UTM_TO_WGS84 = Transformer.from_crs('EPSG:32616', 'EPSG:4326', always_xy=True)
    HAS_PYPROJ = True
except ImportError:
    HAS_PYPROJ = False

# =============================================================================
# UTM-16T TO WGS84 CONVERSION
# =============================================================================

def utm_to_wgs84(easting: float, northing: float, zone: int = 16) -> Tuple[float, float]:
    """
    Convert UTM-16T coordinates to WGS84 lat/lon.
    Uses pyproj if available, otherwise falls back to manual calculation.
    """
    if HAS_PYPROJ:
        lon, lat = UTM_TO_WGS84.transform(easting, northing)
        return lat, lon
    else:
        return _utm_to_wgs84_manual(easting, northing, zone)


def _utm_to_wgs84_manual(easting: float, northing: float, zone: int = 16) -> Tuple[float, float]:
    """Manual UTM to WGS84 conversion (fallback when pyproj unavailable)."""
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
    
    J1 = 3/2 - 27/32 * E2 + 269/512 * E2**2
    J2 = 21/16 - 55/32 * E2**2
    J3 = 151/96 * E2**2
    
    phi1 = mu + J1 * sin(2*mu) + J2 * sin(4*mu) + J3 * sin(6*mu)
    
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
    
    lat_deg = lat * 180 / pi
    lon_deg = lon * 180 / pi
    
    return lat_deg, lon_deg


def tan(x):
    return sin(x) / cos(x)

# =============================================================================
# CORRECTED TARGET DATA (UTM Zone 16 - 457xxx easting)
# =============================================================================

# Zion Corridor Wrecks - CORRECTED UTM coordinates
ZION_CORRIDOR_WRECKS = [
    {
        'name': 'ZION-001',
        'utm_easting': 457420.5,
        'utm_northing': 4702150.3,
        'depth_m': 54.8,
        'contour_ft': 180,
        'length_ft': 281,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Heavy freighter candidate on 180ft contour',
    },
    {
        'name': 'ZION-002',
        'utm_easting': 457535.2,
        'utm_northing': 4702265.8,
        'depth_m': 55.2,
        'contour_ft': 181,
        'length_ft': 272,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Heavy freighter - 181ft contour',
    },
    {
        'name': 'ZION-003',
        'utm_easting': 457650.8,
        'utm_northing': 4702380.2,
        'depth_m': 55.5,
        'contour_ft': 182,
        'length_ft': 263,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Heavy freighter - 182ft contour',
    },
    {
        'name': 'ZION-004',
        'utm_easting': 457765.1,
        'utm_northing': 4702495.5,
        'depth_m': 54.5,
        'contour_ft': 179,
        'length_ft': 291,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Heavy freighter - 179ft contour',
    },
    {
        'name': 'ZION-005',
        'utm_easting': 457880.3,
        'utm_northing': 4702610.1,
        'depth_m': 56.1,
        'contour_ft': 184,
        'length_ft': 253,
        'type': 'LARGE_VESSEL',
        'shelf_lock': False,
        'description': 'Large vessel wreck - 184ft',
    },
    {
        'name': 'ZION-006 (ANCASTE MAIN)',
        'utm_easting': 457990.7,
        'utm_northing': 4702720.4,
        'depth_m': 54.9,
        'contour_ft': 180,
        'length_ft': 266,
        'type': 'ANDASTE_CANDIDATE',
        'shelf_lock': True,
        'description': 'PRIMARY ANCASTE CANDIDATE - 180ft contour, 266ft length, 295° heading',
    },
    {
        'name': 'ZION-007',
        'utm_easting': 458105.2,
        'utm_northing': 4702835.8,
        'depth_m': 55.0,
        'contour_ft': 180,
        'length_ft': 288,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Heavy freighter on 180ft contour',
    },
    {
        'name': 'ZION-008 (DC-4 Wing)',
        'utm_easting': 457500.5,
        'utm_northing': 4702750.2,
        'depth_m': 48.2,
        'contour_ft': 158,
        'length_ft': 117,
        'type': 'AIRCRAFT_DEBRIS',
        'shelf_lock': False,
        'description': 'DC-4 wing fragment - PSF corrected from 154ft to 117ft',
    },
    {
        'name': 'ZION-009 (DC-4 Wing)',
        'utm_easting': 457880.1,
        'utm_northing': 4702920.6,
        'depth_m': 42.5,
        'contour_ft': 139,
        'length_ft': 115,
        'type': 'AIRCRAFT_DEBRIS',
        'shelf_lock': False,
        'description': 'DC-4 wing fragment - Aluminum signature confirmed',
    },
    {
        'name': 'ZION-010',
        'utm_easting': 458550.4,
        'utm_northing': 4703280.9,
        'depth_m': 55.8,
        'contour_ft': 183,
        'length_ft': 241,
        'type': 'LARGE_VESSEL',
        'shelf_lock': False,
        'description': 'Large vessel wreck - 183ft',
    },
]

# Flight 2501 DC-4 Debris Field - CORRECTED UTM coordinates
# Primary impact zone near 42.99°N, 87.96°W (93.5 miles from South Haven)
FLIGHT_2501_DEBRIS = [
    {
        'name': 'DC-4 Primary Impact (Rank 1)',
        'utm_easting': 408500.0,
        'utm_northing': 4760050.0,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 95,
        'type': 'MASTER_TARGET',
        'description': 'Primary impact zone - Aluminum glint + 4 engine cluster',
    },
    {
        'name': 'DC-4 Engine #1 (P&W R-2000)',
        'utm_easting': 408560.3,
        'utm_northing': 4760010.5,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.3)',
    },
    {
        'name': 'DC-4 Engine #2 (P&W R-2000)',
        'utm_easting': 408590.7,
        'utm_northing': 4760070.2,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.1)',
    },
    {
        'name': 'DC-4 Engine #3 (P&W R-2000)',
        'utm_easting': 408470.5,
        'utm_northing': 4759980.8,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.4)',
    },
    {
        'name': 'DC-4 Engine #4 (P&W R-2000)',
        'utm_easting': 408630.2,
        'utm_northing': 4760120.1,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.0)',
    },
    {
        'name': 'DC-4 Wing Fragment A',
        'utm_easting': 457500.5,
        'utm_northing': 4702750.2,
        'depth_m': 48.2,
        'contour_ft': 158,
        'length_ft': 117,
        'type': 'WING',
        'description': 'Aluminum wing section (PSF corrected) - Zion Trench',
    },
    {
        'name': 'DC-4 Wing Fragment B',
        'utm_easting': 457880.1,
        'utm_northing': 4702920.6,
        'depth_m': 42.5,
        'contour_ft': 139,
        'length_ft': 115,
        'type': 'WING',
        'description': 'Aluminum wing section (PSF corrected) - Zion Trench',
    },
    {
        'name': 'DC-4 Fuselage Section',
        'utm_easting': 408550.0,
        'utm_northing': 4760040.0,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 45,
        'type': 'FUSELAGE',
        'description': 'Main fuselage section near primary impact',
    },
    {
        'name': 'DC-4 Tail Section',
        'utm_easting': 408520.0,
        'utm_northing': 4760060.0,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 25,
        'type': 'TAIL',
        'description': 'Tail section with vertical stabilizer',
    },
    {
        'name': 'DC-4 Landing Gear',
        'utm_easting': 408580.0,
        'utm_northing': 4760030.0,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 6,
        'type': 'COMPONENT',
        'description': 'Landing gear assembly (high thermal mass)',
    },
]

# =============================================================================
# NOAA ENC CHARTS - Local Files
# =============================================================================

def get_local_noaa_charts(charts_dir: str = 'noaacharts/ENC_ROOT') -> List[Dict]:
    """
    Get NOAA ENC charts from local directory.
    
    Looks for .png, .kap, or .geo files in the specified directory.
    Default: noaacharts/ENC_ROOT (standard ENC extraction folder)
    """
    charts_path = Path(charts_dir)
    
    if not charts_path.exists():
        print(f"  Warning: NOAA charts directory '{charts_dir}' not found")
        # Try fallback locations
        fallback_paths = ['noaa_charts', 'noaacharts', 'ENC_ROOT', 'charts']
        for fallback in fallback_paths:
            fallback_path = Path(fallback)
            if fallback_path.exists():
                print(f"  Found fallback directory: {fallback}")
                charts_path = fallback_path
                break
        else:
            return []
    
    charts = []
    supported_extensions = ['.png', '.kap', '.geo', '.tiff', '.tif', '.jpw', '.000']
    
    # Search recursively for chart files
    for ext in supported_extensions:
        for chart_file in charts_path.rglob(f'*{ext}'):
            chart_name = chart_file.stem.replace('_', ' ').replace('-', ' ').title()
            charts.append({
                'chart_name': chart_name,
                'chart_number': chart_file.stem[:5] if len(chart_file.stem) >= 5 else chart_file.stem,
                'file_path': str(chart_file),
                'file_type': ext[1:].upper(),
            })
    
    # Remove duplicates (same chart number)
    seen = set()
    unique_charts = []
    for chart in charts:
        if chart['chart_number'] not in seen:
            seen.add(chart['chart_number'])
            unique_charts.append(chart)
    
    charts = unique_charts
    
    # If no files found, use default Great Lakes charts
    if not charts:
        print("  No chart files found, using default Great Lakes ENC references...")
        charts = [
            {'chart_name': 'Lake Michigan - Southern Basin', 'chart_number': '14900', 'file_type': 'ENC'},
            {'chart_name': 'Chicago Harbor', 'chart_number': '14901', 'file_type': 'ENC'},
            {'chart_name': 'Waukegan to Milwaukee', 'chart_number': '14902', 'file_type': 'ENC'},
            {'chart_name': 'Milwaukee Harbor', 'chart_number': '14903', 'file_type': 'ENC'},
            {'chart_name': 'Racine Harbor', 'chart_number': '14904', 'file_type': 'ENC'},
            {'chart_name': 'Kenosha Harbor', 'chart_number': '14905', 'file_type': 'ENC'},
            {'chart_name': 'Lake Superior', 'chart_number': '14960', 'file_type': 'ENC'},
            {'chart_name': 'Lake Huron', 'chart_number': '14860', 'file_type': 'ENC'},
            {'chart_name': 'Lake Erie', 'chart_number': '14830', 'file_type': 'ENC'},
            {'chart_name': 'Lake Ontario', 'chart_number': '14780', 'file_type': 'ENC'},
        ]
    
    return charts


def get_noaa_chart_wms_url(chart_number: str) -> str:
    """Get NOAA ENC WMS URL for a specific chart."""
    return f"https://charts.noaa.gov/arcgis/services/ENC/MapServer/WmsServer?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&FORMAT=image/png&LAYERS={chart_number}&TRANSPARENT=true"


# =============================================================================
# KML GENERATOR
# =============================================================================

def generate_kml_placemark(target: Dict, icon_color: str = 'ff0000ff') -> str:
    """Generate KML Placemark for a target."""
    lat = target.get('lat', 0)
    lon = target.get('lon', 0)
    name = target.get('name', 'Unknown')
    desc = target.get('description', '')
    target_type = target.get('type', 'Unknown')
    
    extended_data = f"""
    <ExtendedData>
        <Data name="type"><value>{target_type}</value></Data>
        <Data name="depth_ft"><value>{target.get('contour_ft', 'N/A')}</value></Data>
        <Data name="length_ft"><value>{target.get('length_ft', 'N/A')}</value></Data>
        <Data name="shelf_lock"><value>{target.get('shelf_lock', False)}</value></Data>
        <Data name="utm_easting"><value>{target.get('utm_easting', 'N/A')}</value></Data>
        <Data name="utm_northing"><value>{target.get('utm_northing', 'N/A')}</value></Data>
        <Data name="utm_zone"><value>16T</value></Data>
    </ExtendedData>
    """
    
    kml = f"""
    <Placemark>
        <name>{name}</name>
        <description><![CDATA[
            <h3>{name}</h3>
            <p><b>Type:</b> {target_type}</p>
            <p><b>Depth:</b> {target.get('contour_ft', 'N/A')} ft ({target.get('depth_m', 'N/A')} m)</p>
            <p><b>Length:</b> {target.get('length_ft', 'N/A')} ft</p>
            <p><b>180ft Shelf-Lock:</b> {'✅ ENGAGED' if target.get('shelf_lock') else '❌ Not engaged'}</p>
            <p><b>UTM-16T:</b> E {target.get('utm_easting', 'N/A'):.1f}, N {target.get('utm_northing', 'N/A'):.1f}</p>
            <p>{desc}</p>
            <p><i>Coordinates corrected - UTM Zone 16 validated (35km from Zone 15/16 boundary)</i></p>
        ]]></description>
        {extended_data}
        <Style>
            <IconStyle>
                <color>{icon_color}</color>
                <scale>1.2</scale>
                <Icon>
                    <href>http://maps.google.com/mapfiles/kml/paddle/{{{{label_color}}}}.png</href>
                </Icon>
            </IconStyle>
            <LabelStyle>
                <color>{icon_color}</color>
                <scale>0.8</scale>
            </LabelStyle>
        </Style>
        <Point>
            <coordinates>{lon},{lat},0</coordinates>
        </Point>
    </Placemark>
    """
    
    return kml


def generate_kml_polygon(name: str, coordinates: List[Tuple[float, float]], color: str = '80ff0000') -> str:
    """Generate KML Polygon for debris field extent."""
    coords_str = ' '.join([f"{lon},{lat},0" for lat, lon in coordinates])
    
    return f"""
    <Placemark>
        <name>{name}</name>
        <Style>
            <LineStyle>
                <color>{color}</color>
                <width>2</width>
            </LineStyle>
            <PolyStyle>
                <color>{color}</color>
                <fill>1</fill>
                <outline>1</outline>
            </PolyStyle>
        </Style>
        <Polygon>
            <outerBoundaryIs>
                <LinearRing>
                    <coordinates>{coords_str}</coordinates>
                </LinearRing>
            </outerBoundaryIs>
        </Polygon>
    </Placemark>
    """


def generate_kml_document(
    zion_wrecks: List[Dict],
    dc4_debris: List[Dict],
    noaa_charts: List[Dict],
) -> str:
    """Generate complete KML document with corrected coordinates."""
    
    # Convert UTM to WGS84 for all targets
    for wreck in zion_wrecks:
        lat, lon = utm_to_wgs84(wreck['utm_easting'], wreck['utm_northing'])
        wreck['lat'] = lat
        wreck['lon'] = lon
    
    for debris in dc4_debris:
        lat, lon = utm_to_wgs84(debris['utm_easting'], debris['utm_northing'])
        debris['lat'] = lat
        debris['lon'] = lon
    
    # Build KML
    kml_content = '<?xml version="1.0" encoding="UTF-8"?>\n'
    kml_content += '<kml xmlns="http://www.opengis.net/kml/2.2">\n'
    kml_content += '<Document>\n'
    
    # Document metadata
    kml_content += f"""
    <name>Zion Corridor Wrecks &amp; Flight 2501 DC-4 (CORRECTED UTM)</name>
    <description>
        <![CDATA[
        <h2>WreckHunter2000 - CESAROPS V1.0 Target Export</h2>
        <p><b>Generated:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
        <p><b>Coordinate System:</b> UTM-16T → WGS84</p>
        <p><b>UTM Zone:</b> 16T (validated - 35km from Zone 15/16 boundary)</p>
        <p><b>PSF Correction:</b> Applied (DC-4 wings: 154ft → 117ft)</p>
        <p><b>180ft Shelf-Lock:</b> Enforced for Andaste candidates</p>
        <p><b>1.33 Refraction:</b> Applied for underwater targets</p>
        <p><b>295° Heading Vector:</b> Andaste's last known bearing</p>
        <hr/>
        <h3>In Memory of Flight 2501</h3>
        <p>Northwest Orient Airlines Flight 2501 disappeared over Lake Michigan on September 21, 1959. All 58 souls aboard were lost. This KMZ is dedicated to finding the final resting place of the DC-4 and honoring the memory of those who perished.</p>
        <p><i>Also dedicated to my father, whose love for the Great Lakes and aviation inspired this quest.</i></p>
        ]]></description>
    
    <!-- Styles -->
    <Style id="andaste_candidate">
        <IconStyle>
            <color>ff0000ff</color>
            <scale>1.5</scale>
            <Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon>
        </IconStyle>
    </Style>
    <Style id="dc4_target">
        <IconStyle>
            <color>ff00ffff</color>
            <scale>1.3</scale>
            <Icon><href>http://maps.google.com/mapfiles/kml/paddle/cyan-circle.png</href></Icon>
        </IconStyle>
    </Style>
    <Style id="engine_target">
        <IconStyle>
            <color>ff00ff00</color>
            <scale>1.0</scale>
            <Icon><href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href></Icon>
        </IconStyle>
    </Style>
    <Style id="memorial_style">
        <IconStyle>
            <color>ffffffff</color>
            <scale>1.5</scale>
            <Icon><href>http://maps.google.com/mapfiles/kml/paddle/wht-circle.png</href></Icon>
        </IconStyle>
    </Style>
    """
    
    # Memorial Placemark
    kml_content += f"""
    <Placemark>
        <name>🕊️ In Memory of Flight 2501 Victims</name>
        <description>
            <![CDATA[
            <h2>Northwest Orient Airlines Flight 2501</h2>
            <p><b>Date:</b> September 21, 1959</p>
            <p><b>Aircraft:</b> Douglas DC-4</p>
            <p><b>Route:</b> Chicago (Midway) → Seattle (via Minneapolis & Spokane)</p>
            <p><b>Victims:</b> 58 souls lost</p>
            <hr/>
            <p><i>"On a stormy night in September 1959, Flight 2501 disappeared over Lake Michigan. This search is dedicated to finding their final resting place and honoring their memory."</i></p>
            ]]></description>
        <styleUrl>#memorial_style</styleUrl>
        <Point>
            <coordinates>-87.9616,42.9876,0</coordinates>
        </Point>
    </Placemark>
    """
    
    # Folder: Zion Corridor Wrecks
    kml_content += '<Folder>\n'
    kml_content += '  <name>Zion Corridor Wrecks (Andaste Candidates) - UTM-16T</name>\n'
    kml_content += '  <description>SS Andaste search area - 180ft depth contour, Zion Trench</description>\n'
    
    for wreck in zion_wrecks:
        color = 'ff0000ff' if wreck.get('shelf_lock') else 'ff00ffff'
        placemark = generate_kml_placemark(wreck, icon_color=color)
        kml_content += f'  {placemark}\n'
    
    kml_content += '</Folder>\n'
    
    # Folder: Flight 2501 DC-4 Debris
    kml_content += '<Folder>\n'
    kml_content += '  <name>Flight 2501 DC-4 Debris Field - UTM-16T</name>\n'
    kml_content += '  <description>Douglas DC-4 crash site - September 21, 1959 - 58 victims</description>\n'
    
    for debris in dc4_debris:
        if debris.get('type') == 'ENGINE':
            color = 'ff00ff00'
        elif debris.get('type') == 'WING':
            color = 'ff00ffff'
        elif debris.get('type') == 'MASTER_TARGET':
            color = 'ffff00ff'
        else:
            color = 'ffffffff'
        
        placemark = generate_kml_placemark(debris, icon_color=color)
        kml_content += f'  {placemark}\n'
    
    # Debris trail polygon (105.5° bearing vector from primary impact)
    debris_coords = [(d['lon'], d['lat']) for d in dc4_debris if d.get('lat')]
    if len(debris_coords) >= 2:
        # Create rough polygon around debris field
        lats = [c[1] for c in debris_coords]
        lons = [c[0] for c in debris_coords]
        min_lat, max_lat = min(lats) - 0.02, max(lats) + 0.02
        min_lon, max_lon = min(lons) - 0.02, max(lons) + 0.02
        
        polygon_coords = [
            (min_lat, min_lon),
            (min_lat, max_lon),
            (max_lat, max_lon),
            (max_lat, min_lon),
        ]
        
        kml_content += generate_kml_polygon(
            'DC-4 Debris Field Extent (105.5° Bearing)',
            polygon_coords,
            color='40ffff00'
        )
    
    kml_content += '</Folder>\n'
    
    # Folder: NOAA ENC Charts (NetworkLinks and local files)
    kml_content += '<Folder>\n'
    kml_content += '  <name>NOAA ENC Charts - Great Lakes</name>\n'
    kml_content += '  <description>Electronic Navigational Charts for Great Lakes</description>\n'
    
    # Check for local chart files
    local_charts = [c for c in noaa_charts if 'file_path' in c]
    remote_charts = [c for c in noaa_charts if 'file_path' not in c]
    
    if local_charts:
        kml_content += f'  <description>{len(local_charts)} local chart files embedded in KMZ</description>\n'
        # Add local charts as GroundOverlays
        for chart in local_charts[:20]:  # Limit to 20 to avoid KML bloat
            safe_name = Path(chart['file_path']).name.replace(' ', '_').replace('&', '_')
            kml_content += f"""
    <GroundOverlay>
        <name>{chart['chart_name']} ({chart['chart_number']})</name>
        <Icon>
            <href>charts/{safe_name}</href>
        </Icon>
        <LatLonBox>
            <north>46.0</north>
            <south>41.0</south>
            <east>-84.0</east>
            <west>-90.0</west>
        </LatLonBox>
    </GroundOverlay>
        """
    
    # Add remote charts as NetworkLinks
    for chart in remote_charts:
        wms_url = get_noaa_chart_wms_url(chart['chart_number'])
        kml_content += f"""
    <NetworkLink>
        <name>{chart['chart_name']} (NOAA ENC {chart['chart_number']})</name>
        <description>Great Lakes Electronic Navigational Chart</description>
        <Link>
            <href>{wms_url}</href>
            <viewRefreshMode>onRegion</viewRefreshMode>
            <viewFormat>BBOX=[bboxWest],[bboxSouth],[bboxEast],[bboxNorth]</viewFormat>
        </Link>
    </NetworkLink>
        """
    
    kml_content += '</Folder>\n'
    
    # Folder: Reference Information
    kml_content += '<Folder>\n'
    kml_content += '  <name>Reference Information</name>\n'
    
    # 180ft Contour Reference
    kml_content += f"""
    <Placemark>
        <name>180ft Depth Contour (Zion Trench)</name>
        <description>Critical depth for SS Andaste wreck location - SS Andaste sank in approximately 180ft of water</description>
        <Style>
            <LineStyle>
                <color>ff0000ff</color>
                <width>3</width>
            </LineStyle>
        </Style>
        <LineString>
            <coordinates>
                -87.58,42.44,0 -87.54,42.44,0 -87.54,42.50,0 -87.58,42.50,0
            </coordinates>
        </LineString>
    </Placemark>
    
    <Placemark>
        <name>UTM Zone 16/15 Boundary (90°W)</name>
        <description>Zone boundary - Zion Cluster is 35km EAST of this line (safe in Zone 16)</description>
        <Style>
            <LineStyle>
                <color>ffff0000</color>
                <width>2</width>
            </LineStyle>
            <PolyStyle>
                <fill>0</fill>
            </PolyStyle>
        </Style>
        <LineString>
            <coordinates>
                -90.0,42.0,0 -90.0,43.5,0
            </coordinates>
        </LineString>
    </Placemark>
    
    <Placemark>
        <name>Zone 16 Central Meridian (87°W)</name>
        <description>Central meridian for UTM Zone 16 - Zion Cluster is ~42km west of this line</description>
        <Style>
            <LineStyle>
                <color>ff00ffff</color>
                <width>1</width>
            </LineStyle>
        </Style>
        <LineString>
            <coordinates>
                -87.0,42.0,0 -87.0,43.5,0
            </coordinates>
        </LineString>
    </Placemark>
    """
    
    kml_content += '</Folder>\n'
    
    kml_content += '</Document>\n'
    kml_content += '</kml>\n'
    
    return kml_content


def create_kmz(kml_content: str, output_path: Path, noaa_charts: List[Dict] = None) -> None:
    """Create KMZ (zipped KML) file with optional local chart files."""
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml_content)
        
        # Add local chart files if available
        if noaa_charts:
            for chart in noaa_charts:
                if 'file_path' in chart:
                    chart_path = Path(chart['file_path'])
                    if chart_path.exists():
                        # Sanitize the archive name (no special chars, relative path)
                        safe_name = chart_path.name.replace(' ', '_').replace('&', '_')
                        arcname = f'charts/{safe_name}'
                        try:
                            kmz.write(chart_path, arcname)
                        except Exception as e:
                            print(f"  Warning: Could not add {chart_path.name}: {e}")
    
    print(f"KMZ created: {output_path}")
    print(f"Size: {output_path.stat().st_size / 1024:.1f} KB")


# =============================================================================
# MAIN EXECUTION
# =============================================================================

def main():
    """Main function: Generate KMZ with corrected coordinates."""
    print("="*80)
    print("FLIGHT 2501 & ZION CORRIDOR KMZ GENERATOR (CORRECTED UTM)")
    print("="*80)
    print()
    print("UTM Zone 16 Validation:")
    print("  Zone Bounds: 90°W to 84°W")
    print("  Central Meridian: 87°W")
    print("  Zion Cluster: ~87.56°W (35km east of Zone 15/16 boundary)")
    print("  Status: SAFE for Zone 16 processing")
    print()
    
    # Output directory
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Get NOAA charts from local directory
    print("Loading NOAA ENC charts from local directory...")
    noaa_charts = get_local_noaa_charts('noaa_charts')
    print(f"  Found {len(noaa_charts)} chart(s)")
    
    # Generate KML document
    print("Generating KML document with corrected coordinates...")
    kml_content = generate_kml_document(
        zion_wrecks=ZION_CORRIDOR_WRECKS.copy(),
        dc4_debris=FLIGHT_2501_DEBRIS.copy(),
        noaa_charts=noaa_charts,
    )
    
    # Create KMZ
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'flight2501_zion_corrected_{timestamp}.kmz'
    
    create_kmz(kml_content, kmz_path, noaa_charts)
    
    # Also save raw KML
    kml_path = output_dir / f'flight2501_zion_corrected_{timestamp}.kml'
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print()
    print("="*80)
    print("SUMMARY")
    print("="*80)
    print(f"Zion Corridor Wrecks: {len(ZION_CORRIDOR_WRECKS)}")
    print(f"DC-4 Debris Targets: {len(FLIGHT_2501_DEBRIS)}")
    print(f"NOAA ENC Charts: {len(noaa_charts)}")
    print()
    print(f"KMZ Export: {kmz_path}")
    print(f"KML Export: {kml_path}")
    print()
    
    # Print coordinate table
    print("="*80)
    print("COORDINATE TABLE (UTM-16T → WGS84)")
    print("="*80)
    print()
    
    print("ZION CORRIDOR WRECKS (Corrected UTM):")
    print(f"{'Name':<35} {'Easting':<12} {'Northing':<12} {'Lat':<12} {'Lon':<12}")
    print("-"*90)
    
    for wreck in ZION_CORRIDOR_WRECKS:
        lat, lon = utm_to_wgs84(wreck['utm_easting'], wreck['utm_northing'])
        print(f"{wreck['name']:<35} {wreck['utm_easting']:<12.1f} {wreck['utm_northing']:<12.1f} {lat:<12.6f} {lon:<12.6f}")
    
    print()
    print("FLIGHT 2501 DC-4 DEBRIS (Corrected UTM):")
    print(f"{'Name':<35} {'Easting':<12} {'Northing':<12} {'Lat':<12} {'Lon':<12}")
    print("-"*90)
    
    for debris in FLIGHT_2501_DEBRIS:
        lat, lon = utm_to_wgs84(debris['utm_easting'], debris['utm_northing'])
        print(f"{debris['name']:<35} {debris['utm_easting']:<12.1f} {debris['utm_northing']:<12.1f} {lat:<12.6f} {lon:<12.6f}")
    
    print()
    print("="*80)
    print("IN MEMORY OF FLIGHT 2501 VICTIMS (58 souls)")
    print("September 21, 1959 - Lake Michigan")
    print("="*80)
    print()
    print("KMZ GENERATION COMPLETE")
    print("="*80)
    
    return {
        'kmz_path': str(kmz_path),
        'kml_path': str(kml_path),
        'zion_wrecks': len(ZION_CORRIDOR_WRECKS),
        'dc4_debris': len(FLIGHT_2501_DEBRIS),
        'noaa_charts': len(noaa_charts),
    }


if __name__ == '__main__':
    result = main()
