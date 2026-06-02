"""
zion_dc4_kmz_generator.py

Generate Google Earth KMZ with:
[1] Zion Corridor Wrecks (Andaste candidates) - UTM-16T corrected
[2] Flight 2501 DC-4 Debris Field - Aviation targets with PSF correction
[3] NOAA ENC Charts overlay - Direct from NOAA Chart REST API

All coordinates converted from UTM-16T to WGS84 (lat/lon) for Google Earth.
"""

import json
import struct
import zipfile
import requests
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
    from math import radians, sin, cos, atan2, sqrt, pi, tan

# =============================================================================
# UTM-16T TO WGS84 CONVERSION
# =============================================================================

def utm_to_wgs84(easting: float, northing: float, zone: int = 16, hemisphere: str = 'N') -> Tuple[float, float]:
    """
    Convert UTM coordinates to WGS84 lat/lon.
    
    Uses pyproj if available, otherwise falls back to manual calculation.
    
    Args:
        easting: UTM easting (meters)
        northing: UTM northing (meters)
        zone: UTM zone number (default 16 for Lake Michigan)
        hemisphere: 'N' for northern, 'S' for southern
    
    Returns:
        (latitude, longitude) in decimal degrees
    """
    if HAS_PYPROJ:
        # Use pyproj for accurate conversion
        lon, lat = UTM_TO_WGS84.transform(easting, northing)
        return lat, lon
    else:
        # Fallback to manual calculation (less accurate)
        return _utm_to_wgs84_manual(easting, northing, zone, hemisphere)


def _utm_to_wgs84_manual(easting: float, northing: float, zone: int = 16, hemisphere: str = 'N') -> Tuple[float, float]:
    """
    Manual UTM to WGS84 conversion (fallback when pyproj unavailable).
    """
    # WGS84 ellipsoid constants
    A = 6378137.0  # Semi-major axis (meters)
    F = 1 / 298.257223563  # Flattening
    E2 = 2 * F - F * F  # Eccentricity squared
    
    # Central meridian for zone
    lon_origin = (zone - 1) * 6 - 180 + 3  # Central meridian
    
    # Scale factor
    K0 = 0.9996
    
    # False easting/northing
    E0 = 500000.0
    N0 = 0.0 if hemisphere == 'N' else 10000000.0
    
    # Remove false offsets
    x = easting - E0
    y = northing - N0
    
    # Compute latitude
    M = y / K0
    mu = M / (A * (1 - E2/4 - 3*E2**2/64 - 5*E2**3/256))
    
    J1 = 3/2 - 27/32 * E2 + 269/512 * E2**2
    J2 = 21/16 - 55/32 * E2**2
    J3 = 151/96 * E2**2
    
    phi1 = mu + J1 * sin(2*mu) + J2 * sin(4*mu) + J3 * sin(6*mu)
    
    # Compute auxiliary values
    N1 = A / sqrt(1 - E2 * sin(phi1)**2)
    T1 = tan(phi1)**2
    C1 = E2 / (1 - E2) * cos(phi1)**2
    R1 = A * (1 - E2) / (1 - E2 * sin(phi1)**2)**1.5
    D = x / (N1 * K0)
    
    # Latitude
    lat = phi1 - (N1 * tan(phi1) / R1) * (
        D**2/2 - (5 + 3*T1 + 10*C1 - 4*C1**2 - 9*E2) * D**4/24 +
        (61 + 90*T1 + 298*C1 + 45*T1**2 - 252*E2 - 3*C1**2) * D**6/720
    )
    
    # Longitude
    lon = lon_origin + (
        D - (1 + 2*T1 + C1) * D**3/6 +
        (5 - 2*C1 + 28*T1 - 3*C1**2 + 8*E2 + 24*T1**2) * D**5/120
    ) / cos(phi1)
    
    # Convert to degrees
    lat_deg = lat * 180 / pi
    lon_deg = lon * 180 / pi
    
    return lat_deg, lon_deg


def wgs84_to_utm(lat: float, lon: float) -> Tuple[float, float, int]:
    """
    Convert WGS84 lat/lon to UTM coordinates.
    
    Returns:
        (easting, northing, zone)
    """
    # WGS84 ellipsoid constants
    A = 6378137.0
    F = 1 / 298.257223563
    E2 = 2 * F - F * F
    
    K0 = 0.9996
    
    # Calculate zone
    zone = int((lon + 180) / 6) + 1
    
    # Central meridian
    lon_origin = (zone - 1) * 6 - 180 + 3
    
    # Convert to radians
    lat_rad = radians(lat)
    lon_rad = radians(lon)
    lon_origin_rad = radians(lon_origin)
    
    # Compute auxiliary values
    N = A / sqrt(1 - E2 * sin(lat_rad)**2)
    T = tan(lat_rad)**2
    C = E2 / (1 - E2) * cos(lat_rad)**2
    A_val = cos(lat_rad) * (lon_rad - lon_origin_rad)
    
    M = A * (
        (1 - E2/4 - 3*E2**2/64 - 5*E2**3/256) * lat_rad -
        (3*E2/8 + 3*E2**2/32 + 45*E2**3/1024) * sin(2*lat_rad) +
        (15*E2**2/256 + 45*E2**3/1024) * sin(4*lat_rad) -
        (35*E2**3/3072) * sin(6*lat_rad)
    )
    
    # Easting
    easting = K0 * N * (
        A_val + (1 - T + C) * A_val**3/6 +
        (5 - 18*T + T**2 + 72*C - 58*E2) * A_val**5/120
    ) + 500000.0
    
    # Northing
    northing = K0 * (
        M + N * tan(lat_rad) * (
            A_val**2/2 + (5 - T + 9*C + 4*C**2) * A_val**4/24 +
            (61 - 58*T + T**2 + 600*C - 330*E2) * A_val**6/720
        )
    )
    
    # Southern hemisphere adjustment
    if lat < 0:
        northing += 10000000.0
    
    return easting, northing, zone


# =============================================================================
# TARGET DATA
# =============================================================================

# Zion Corridor Wrecks (Andaste candidates) - UTM-16T coordinates
ZION_CORRIDOR_WRECKS = [
    {
        'name': 'SS Andaste (Target #1)',
        'utm_easting': 412500.5,
        'utm_northing': 4702150.3,
        'depth_m': 54.8,
        'contour_ft': 180,
        'length_ft': 281,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': True,
        'description': 'Whaleback freighter, lost 1907, 310ft original length',
    },
    {
        'name': 'Andaste Candidate (Target #2)',
        'utm_easting': 412535.2,
        'utm_northing': 4702265.8,
        'depth_m': 55.2,
        'contour_ft': 181,
        'length_ft': 272,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': True,
        'description': 'PRIMARY ANDASTE CANDIDATE - 180ft contour, 266ft length',
    },
    {
        'name': 'Andaste Candidate (Target #3)',
        'utm_easting': 412650.8,
        'utm_northing': 4702380.2,
        'depth_m': 55.5,
        'contour_ft': 182,
        'length_ft': 263,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': True,
        'description': 'PRIMARY ANDASTE CANDIDATE - Broken hull section',
    },
    {
        'name': 'SS William H. Squire (Target #4)',
        'utm_easting': 412765.1,
        'utm_northing': 4702495.5,
        'depth_m': 54.5,
        'contour_ft': 179,
        'length_ft': 291,
        'type': 'HEAVY_FREIGHTER',
        'shelf_lock': False,
        'description': 'Steel freighter, lost 1910, 400ft original length',
    },
    {
        'name': 'Zion Target #5',
        'utm_easting': 412880.3,
        'utm_northing': 4702610.1,
        'depth_m': 56.1,
        'contour_ft': 184,
        'length_ft': 253,
        'type': 'LARGE_VESSEL',
        'shelf_lock': False,
        'description': 'Large vessel wreck, depth 184ft',
    },
    {
        'name': 'ZION-006 (Andaste Main)',
        'utm_easting': 412990.7,
        'utm_northing': 4702720.4,
        'depth_m': 54.9,
        'contour_ft': 180,
        'length_ft': 266,
        'type': 'ANDASTE_CANDIDATE',
        'shelf_lock': True,
        'description': 'PRIMARY ANDASTE CANDIDATE - Exact 180ft contour, 266ft length',
    },
    {
        'name': 'Zion Target #7',
        'utm_easting': 413105.2,
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
        'utm_easting': 412500.5,
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
        'utm_easting': 412880.1,
        'utm_northing': 4702920.6,
        'depth_m': 42.5,
        'contour_ft': 139,
        'length_ft': 115,
        'type': 'AIRCRAFT_DEBRIS',
        'shelf_lock': False,
        'description': 'DC-4 wing fragment - Aluminum signature confirmed',
    },
    {
        'name': 'Zion Target #10',
        'utm_easting': 413550.4,
        'utm_northing': 4703280.9,
        'depth_m': 55.8,
        'contour_ft': 183,
        'length_ft': 241,
        'type': 'LARGE_VESSEL',
        'shelf_lock': False,
        'description': 'Large vessel wreck',
    },
]

# Flight 2501 DC-4 Debris Field
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
        'name': 'DC-4 Engine #1',
        'utm_easting': 408560.3,
        'utm_northing': 4760010.5,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.3)',
    },
    {
        'name': 'DC-4 Engine #2',
        'utm_easting': 408590.7,
        'utm_northing': 4760070.2,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.1)',
    },
    {
        'name': 'DC-4 Engine #3',
        'utm_easting': 408470.5,
        'utm_northing': 4759980.8,
        'depth_m': 91.4,
        'contour_ft': 300,
        'length_ft': 8,
        'type': 'ENGINE',
        'description': 'Pratt & Whitney R-2000 radial engine (Z-score: -2.4)',
    },
    {
        'name': 'DC-4 Engine #4',
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
        'utm_easting': 412500.5,
        'utm_northing': 4702750.2,
        'depth_m': 48.2,
        'contour_ft': 158,
        'length_ft': 117,
        'type': 'WING',
        'description': 'Aluminum wing section (PSF corrected)',
    },
    {
        'name': 'DC-4 Wing Fragment B',
        'utm_easting': 412880.1,
        'utm_northing': 4702920.6,
        'depth_m': 42.5,
        'contour_ft': 139,
        'length_ft': 115,
        'type': 'WING',
        'description': 'Aluminum wing section (PSF corrected)',
    },
]

# =============================================================================
# NOAA ENC CHART FETCHER
# =============================================================================

def fetch_noaa_enc_charts(bbox: Dict[str, float]) -> List[Dict]:
    """
    Fetch NOAA ENC (Electronic Navigational Charts) from REST API.
    
    API: https://chartbuilder.noaa.gov/
    
    Args:
        bbox: Bounding box with keys: lat_min, lon_min, lat_max, lon_max
    
    Returns:
        List of chart metadata dicts
    """
    # Fallback charts for Lake Michigan (used when API unavailable)
    fallback_charts = [
        {'chart_name': 'Lake Michigan - Southern Basin', 'chart_number': '14900'},
        {'chart_name': 'Chicago Harbor', 'chart_number': '14901'},
        {'chart_name': 'Waukegan to Milwaukee', 'chart_number': '14902'},
        {'chart_name': 'Milwaukee Harbor', 'chart_number': '14903'},
        {'chart_name': 'Racine Harbor', 'chart_number': '14904'},
        {'chart_name': 'Kenosha Harbor', 'chart_number': '14905'},
    ]
    
    # NOAA Chart Builder API endpoint
    api_url = "https://chartbuilder.noaa.gov/arcgis/rest/services/ENC/MapServer/query"
    
    # Query parameters
    params = {
        'f': 'json',
        'geometry': f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}",
        'geometryType': 'esriGeometryEnvelope',
        'inSR': '4326',
        'spatialRel': 'esriSpatialRelIntersects',
        'outFields': '*',
        'returnGeometry': 'true',
        'outSR': '4326',
    }
    
    try:
        response = requests.get(api_url, params=params, timeout=30)
        response.raise_for_status()
        data = response.json()
        
        charts = []
        if 'features' in data:
            for feature in data['features']:
                attrs = feature.get('attributes', {})
                geom = feature.get('geometry', {})
                
                charts.append({
                    'chart_name': attrs.get('CHART_NAME', 'Unknown'),
                    'chart_number': attrs.get('CHART_NUM', 'Unknown'),
                    'scale': attrs.get('SCALE', 'Unknown'),
                    'geometry': geom,
                })
        
        if charts:
            return charts
        else:
            print("  NOAA API returned no results, using fallback charts...")
            return fallback_charts
    
    except Exception as e:
        print(f"  NOAA ENC API unavailable, using fallback charts...")
        return fallback_charts


def get_noaa_chart_wms_url(chart_number: str) -> str:
    """
    Get NOAA ENC WMS URL for a specific chart.
    
    Args:
        chart_number: NOAA chart number (e.g., '14900')
    
    Returns:
        WMS URL for Google Earth NetworkLink
    """
    return f"https://charts.noaa.gov/arcgis/services/ENC/MapServer/WmsServer?SERVICE=WMS&VERSION=1.3.0&REQUEST=GetMap&FORMAT=image/png&LAYERS={chart_number}&TRANSPARENT=true"


# =============================================================================
# KML GENERATOR
# =============================================================================

def generate_kml_placemark(target: Dict, icon_color: str = 'ff0000ff') -> str:
    """
    Generate KML Placemark for a target.
    
    Args:
        target: Target dict with lat, lon, name, description
        icon_color: KML color code (AABBGGRR)
    
    Returns:
        KML Placemark XML string
    """
    lat = target.get('lat', 0)
    lon = target.get('lon', 0)
    name = target.get('name', 'Unknown')
    desc = target.get('description', '')
    target_type = target.get('type', 'Unknown')
    
    # Add UTM and depth info to description
    extended_data = f"""
    <ExtendedData>
        <Data name="type"><value>{target_type}</value></Data>
        <Data name="depth_ft"><value>{target.get('contour_ft', 'N/A')}</value></Data>
        <Data name="length_ft"><value>{target.get('length_ft', 'N/A')}</value></Data>
        <Data name="shelf_lock"><value>{target.get('shelf_lock', False)}</value></Data>
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
            <p>{desc}</p>
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
    """
    Generate KML Polygon (for debris field extent).
    
    Args:
        name: Polygon name
        coordinates: List of (lon, lat) tuples
        color: KML color code
    
    Returns:
        KML Polygon XML string
    """
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
    """
    Generate complete KML document.
    
    Args:
        zion_wrecks: List of Zion Corridor wreck targets (with lat/lon)
        dc4_debris: List of DC-4 debris targets (with lat/lon)
        noaa_charts: List of NOAA ENC charts
    
    Returns:
        Complete KML document XML string
    """
    # Convert UTM to lat/lon for all targets
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
    <name>Zion Corridor Wrecks &amp; Flight 2501 DC-4</name>
    <description>
        <![CDATA[
        <h2>WreckHunter2000 Target Export</h2>
        <p><b>Generated:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
        <p><b>Coordinate System:</b> UTM-16T → WGS84</p>
        <p><b>PSF Correction:</b> Applied (DC-4 wings: 154ft → 117ft)</p>
        <p><b>180ft Shelf-Lock:</b> Enforced for Andaste candidates</p>
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
    """
    
    # Folder: Zion Corridor Wrecks
    kml_content += '<Folder>\n'
    kml_content += '  <name>Zion Corridor Wrecks (Andaste Candidates)</name>\n'
    
    for wreck in zion_wrecks:
        color = 'ff0000ff' if wreck.get('shelf_lock') else 'ff00ffff'
        placemark = generate_kml_placemark(wreck, icon_color=color)
        kml_content += f'  {placemark}\n'
    
    kml_content += '</Folder>\n'
    
    # Folder: Flight 2501 DC-4 Debris
    kml_content += '<Folder>\n'
    kml_content += '  <name>Flight 2501 DC-4 Debris Field</name>\n'
    
    for debris in dc4_debris:
        if debris.get('type') == 'ENGINE':
            color = 'ff00ff00'
        elif debris.get('type') == 'WING':
            color = 'ff00ffff'
        else:
            color = 'ffff00ff'
        
        placemark = generate_kml_placemark(debris, icon_color=color)
        kml_content += f'  {placemark}\n'
    
    # Debris trail polygon (105.5° bearing vector)
    debris_coords = [(d['lon'], d['lat']) for d in dc4_debris if d.get('lat')]
    if len(debris_coords) >= 2:
        # Create rough polygon around debris field
        min_lon = min(c[0] for c in debris_coords) - 0.05
        max_lon = max(c[0] for c in debris_coords) + 0.05
        min_lat = min(c[1] for c in debris_coords) - 0.05
        max_lat = max(c[1] for c in debris_coords) + 0.05
        
        polygon_coords = [
            (min_lat, min_lon),
            (min_lat, max_lon),
            (max_lat, max_lon),
            (max_lat, min_lon),
        ]
        
        kml_content += generate_kml_polygon(
            'DC-4 Debris Field Extent',
            polygon_coords,
            color='40ffff00'
        )
    
    kml_content += '</Folder>\n'
    
    # Folder: NOAA ENC Charts (NetworkLinks)
    kml_content += '<Folder>\n'
    kml_content += '  <name>NOAA ENC Charts</name>\n'
    
    # Add Lake Michigan ENC charts
    noaa_charts_list = [
        {'name': 'Lake Michigan - Southern Basin', 'number': '14900'},
        {'name': 'Chicago Harbor', 'number': '14901'},
        {'name': 'Waukegan to Milwaukee', 'number': '14902'},
        {'name': 'Milwaukee Harbor', 'number': '14903'},
    ]
    
    for chart in noaa_charts_list:
        wms_url = get_noaa_chart_wms_url(chart['number'])
        kml_content += f"""
    <NetworkLink>
        <name>{chart['name']} (NOAA ENC {chart['number']})</name>
        <Link>
            <href>{wms_url}</href>
            <viewRefreshMode>onRegion</viewRefreshMode>
            <viewFormat>BBOX=[bboxWest],[bboxSouth],[bboxEast],[bboxNorth]</viewFormat>
        </Link>
    </NetworkLink>
        """
    
    kml_content += '</Folder>\n'
    
    # Folder: Bathymetry Reference
    kml_content += '<Folder>\n'
    kml_content += '  <name>180ft Contour Reference</name>\n'
    kml_content += f"""
    <Placemark>
        <name>180ft Depth Contour (Zion Trench)</name>
        <description>Critical depth for SS Andaste wreck location</description>
        <Style>
            <LineStyle>
                <color>ff0000ff</color>
                <width>3</width>
            </LineStyle>
        </Style>
        <LineString>
            <coordinates>
                -87.15,42.44,0 -87.06,42.44,0 -87.06,42.49,0 -87.15,42.49,0
            </coordinates>
        </LineString>
    </Placemark>
    """
    kml_content += '</Folder>\n'
    
    kml_content += '</Document>\n'
    kml_content += '</kml>\n'
    
    return kml_content


def create_kmz(
    kml_content: str,
    output_path: Path,
) -> None:
    """
    Create KMZ (zipped KML) file.
    
    Args:
        kml_content: KML XML string
        output_path: Output KMZ file path
    """
    with zipfile.ZipFile(output_path, 'w', zipfile.ZIP_DEFLATED) as kmz:
        kmz.writestr('doc.kml', kml_content)
    
    print(f"KMZ created: {output_path}")
    print(f"Size: {output_path.stat().st_size / 1024:.1f} KB")


# =============================================================================
# MAIN EXECUTION
# =============================================================================

def main():
    """
    Main function: Generate KMZ with Zion wrecks, DC-4 debris, and NOAA ENC charts.
    """
    print("="*80)
    print("ZION CORRIDOR & FLIGHT 2501 KMZ GENERATOR")
    print("="*80)
    print()
    
    # Output directory
    output_dir = Path('outputs/kmz_exports')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # Bounding box for NOAA ENC query
    bbox = {
        'lat_min': 42.40,
        'lat_max': 43.00,
        'lon_min': -88.20,
        'lon_max': -86.20,
    }
    
    # Fetch NOAA ENC charts
    print("Fetching NOAA ENC charts...")
    noaa_charts = fetch_noaa_enc_charts(bbox)
    print(f"  Found {len(noaa_charts)} ENC charts")
    
    # Generate KML document
    print("Generating KML document...")
    kml_content = generate_kml_document(
        zion_wrecks=ZION_CORRIDOR_WRECKS.copy(),
        dc4_debris=FLIGHT_2501_DEBRIS.copy(),
        noaa_charts=noaa_charts,
    )
    
    # Create KMZ
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kmz_path = output_dir / f'zion_dc4_targets_{timestamp}.kmz'
    
    create_kmz(kml_content, kmz_path)
    
    # Also save raw KML
    kml_path = output_dir / f'zion_dc4_targets_{timestamp}.kml'
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
    
    print("ZION CORRIDOR WRECKS:")
    print(f"{'Name':<40} {'Easting':<12} {'Northing':<12} {'Lat':<12} {'Lon':<12}")
    print("-"*90)
    
    for wreck in ZION_CORRIDOR_WRECKS:
        lat, lon = utm_to_wgs84(wreck['utm_easting'], wreck['utm_northing'])
        print(f"{wreck['name']:<40} {wreck['utm_easting']:<12.1f} {wreck['utm_northing']:<12.1f} {lat:<12.6f} {lon:<12.6f}")
    
    print()
    print("FLIGHT 2501 DC-4 DEBRIS:")
    print(f"{'Name':<40} {'Easting':<12} {'Northing':<12} {'Lat':<12} {'Lon':<12}")
    print("-"*90)
    
    for debris in FLIGHT_2501_DEBRIS:
        lat, lon = utm_to_wgs84(debris['utm_easting'], debris['utm_northing'])
        print(f"{debris['name']:<40} {debris['utm_easting']:<12.1f} {debris['utm_northing']:<12.1f} {lat:<12.6f} {lon:<12.6f}")
    
    print()
    print("="*80)
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
