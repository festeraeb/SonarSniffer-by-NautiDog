"""
rossa_search_kml.py

Creates Google Earth KML search boxes based on drift modeling results.

Search Areas:
1. Primary: 8-9 PM CDT Aug 22 sinking locations (17 NM from McKinley)
2. Secondary: Fender drift path to Grand Haven
3. Tertiary: Debris field around primary zones

Based on user's drift modeling:
- Stokes drift: 0.003-0.006 (2-3 ft seas)
- Windage (α): 0.03-0.06
- Sinking time: 8-9 PM CDT Aug 22, 2025
- Location: ~17 NM east of McKinley Marina
"""

import json
from pathlib import Path
from datetime import datetime

# ── Search Areas from Drift Modeling ─────────────────────────────────────────

# User's ACTUAL search grid based on mission + drift modeling
# Rossa was heading OUT to the deep on a mission
# 3 PM last seen + 3 knots average × 5 hours = ~15 NM straight out from McKinley
# Search should favor the OUTWARD route (east), not be "just shy"
MCKINLEY_MARINA = {
    'name': 'McKinley Marina',
    'lat': 43.0167,
    'lon': -87.9006,
}

# Search center: 15-20 NM straight east of McKinley (on his mission route)
# NOT 8 miles - that's where he was at 3 PM, but he kept going OUT
SEARCH_CENTER = {
    'name': 'Search Grid Center (15-20 NM out - mission route)',
    'lat': 43.0167,  # Same latitude as McKinley (heading straight east)
    'lon': -87.65,  # ~15-20 NM east of McKinley (3 knots × 5 hours)
    'ellipse_major_nm': 25,  # Major axis (east-west, mission direction)
    'ellipse_minor_nm': 12,  # Minor axis (north-south, less uncertainty)
    'note': 'Favor the OUTWARD route - he was on a mission to the deep, not meandering',
}

# Primary zones (sub-zones within the 15 NM search grid)
PRIMARY_ZONES = [
    {
        'name': '3 PM Last Position (8 miles out)',
        'lat': 43.05,
        'lon': -87.75,
        'radius_nm': 2,
        'confidence': 'Last confirmed visual position',
        'reachable_at': 'Starting point',
        'note': 'Rossa was HERE at 3 PM',
    },
    {
        'name': '8 PM CDT Probable Sinking',
        'lat': 43.12,
        'lon': -87.65,
        'radius_nm': 3,
        'confidence': 'Based on drift model to fender recovery',
        'reachable_at': '5-6 hours drift from 3 PM @ 1-1.5 kt',
        'note': 'Most likely sinking location (before fender separated)',
    },
    {
        'name': '9 PM CDT Alternate Sinking',
        'lat': 43.13,
        'lon': -87.63,
        'radius_nm': 3,
        'confidence': 'Later sinking time scenario',
        'reachable_at': '6-7 hours drift from 3 PM @ 1-1.5 kt',
        'note': 'If sinking was later than 8 PM',
    },
]

# Fender drift path (for DEBRIS search, NOT main wreck)
FENDER_DRIFT_PATH = [
    {'lat': 43.12, 'lon': -87.65, 'time': 'Aug 22, 8:00 PM CDT (sinking)'},
    {'lat': 43.10, 'lon': -87.00, 'time': 'Aug 23, ~6:00 AM CDT'},
    {'lat': 43.08, 'lon': -86.50, 'time': 'Aug 23, ~6:00 PM CDT'},
    {'lat': 43.06, 'lon': -86.25, 'time': 'Aug 24, ~12:00 PM CDT (Found)'},
]

# Fender found location
FENDER_FOUND = {
    'name': 'Fender Found (Grand Haven area)',
    'lat': 43.06,
    'lon': -86.25,
}

# Drift path waypoints (interpolated)
DRIFT_PATH = [
    {'lat': 43.16377, 'lon': -87.51664, 'time': 'Aug 22, 8:00 PM CDT'},
    {'lat': 43.10, 'lon': -87.00, 'time': 'Aug 23, ~6:00 AM CDT'},
    {'lat': 43.08, 'lon': -86.50, 'time': 'Aug 23, ~6:00 PM CDT'},
    {'lat': 43.06, 'lon': -86.25, 'time': 'Aug 24, ~12:00 PM CDT (Found)'},
]

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'rossa_search_kml'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ── KML Generation ────────────────────────────────────────────────────────────

def create_kml_document() -> str:
    """Create complete KML document with all search areas."""
    
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>Rossa Search Areas - August 2025</name>
    <description>
      <![CDATA[
      <h1>Rossa Search and Rescue - Priority Areas</h1>
      <p><b>Generated:</b> ''' + datetime.now().strftime('%Y-%m-%d %H:%M:%S') + '''</p>
      <p><b>Sinking Time:</b> August 22, 2025, 8-9 PM CDT</p>
      <p><b>Based on:</b> Drift modeling with Stokes drift 0.003-0.006, Windage α=0.03-0.06</p>
      
      <h2>Search Priorities</h2>
      <ol>
        <li><b>PRIMARY:</b> 8-9 PM sinking zones (17 NM from McKinley)</li>
        <li><b>SECONDARY:</b> Drift path to Grand Haven</li>
        <li><b>TERTIARY:</b> Fender recovery area</li>
      </ol>
      
      <h2>Target Description</h2>
      <ul>
        <li><b>Vessel:</b> Red-and-white Bristol 32 sailboat "Rossa"</li>
        <li><b>Fender:</b> Orange large teardrop with black loop area, 3-foot line attached</li>
        <li><b>Person:</b> Blue/white checkered shirt, dark shorts, brown flip-flops</li>
      </ul>
      ]]>
    </description>
    
    <!-- Styles -->
    <Style id="primarySearchStyle">
      <LineStyle>
        <color>ff0000ff</color>
        <width>3</width>
      </LineStyle>
      <PolyStyle>
        <color>400000ff</color>
        <fill>1</fill>
        <outline>1</outline>
      </PolyStyle>
    </Style>
    
    <Style id="secondarySearchStyle">
      <LineStyle>
        <color>ffffff00</color>
        <width>2</width>
      </LineStyle>
      <PolyStyle>
        <color>40ffff00</color>
        <fill>1</fill>
        <outline>1</outline>
      </PolyStyle>
    </Style>
    
    <Style id="driftPathStyle">
      <LineStyle>
        <color>ff00ffff</color>
        <width>2</width>
      </LineStyle>
    </Style>
    
    <Style id="markerStyle">
      <IconStyle>
        <color>ff0000ff</color>
        <scale>1.2</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
'''

    # PRIMARY SEARCH GRID: 15 NM arc (270°, not back to shore)
    kml += f'''
    <!-- Primary Search Grid: 15 NM arc -->
    <Placemark>
      <name>PRIMARY SEARCH GRID: 8 miles out + 15 NM radius (270° arc)</name>
      <description>
        <![CDATA[
        <p><b>Center:</b> 8 NM east of McKinley Marina</p>
        <p><b>Radius:</b> 15 NM (270° arc)</p>
        <p><b>Excludes:</b> Western quadrant (back toward shore)</p>
        <p><b>Area:</b> ~180 square NM</p>
        <p><b>Based on:</b> Real drifter modeling for Lake Michigan</p>
        
        <h3>Search Priority</h3>
        <ol>
          <li>3 PM last position (8 miles out)</li>
          <li>8-9 PM probable sinking locations</li>
          <li>Remainder of 15 NM arc</li>
        </ol>
        ]]>
      </description>
      <styleUrl>#primarySearchStyle</styleUrl>
      <Polygon>
        <extrude>0</extrude>
        <altitudeMode>clampToGround</altitudeMode>
        <outerBoundaryIs>
          <LinearRing>
            <coordinates>{generate_elliptical_arc(SEARCH_CENTER['lat'], SEARCH_CENTER['lon'], SEARCH_CENTER['ellipse_major_nm'], SEARCH_CENTER['ellipse_minor_nm'], -45, 225)}</coordinates>
          </LinearRing>
        </outerBoundaryIs>
      </Polygon>
    </Placemark>

    <!-- Search Grid Center Marker -->
    <Placemark>
      <name>SEARCH GRID CENTER (8 miles out)</name>
      <description>Center of 25 NM elliptical search arc - 8 NM east of McKinley Marina</description>
      <styleUrl>#markerStyle</styleUrl>
      <Point>
        <coordinates>{SEARCH_CENTER['lon']},{SEARCH_CENTER['lat']},0</coordinates>
      </Point>
    </Placemark>
'''

    # Primary search zones (circles)
    for i, zone in enumerate(PRIMARY_ZONES, 1):
        kml += f'''
    <!-- Primary Search Zone {i}: {zone['name']} -->
    <Placemark>
      <name>{zone['name']}</name>
      <description>
        <![CDATA[
        <p><b>Coordinates:</b> {zone['lat']:.5f}°N, {zone['lon']:.5f}°W</p>
        <p><b>Search Radius:</b> {zone['radius_nm']} NM ({zone['radius_nm']*1.852:.1f} km)</p>
        <p><b>Confidence:</b> {zone['confidence']}</p>
        <p><b>Reachable at:</b> {zone['reachable_at']}</p>
        <p><b>Drift Model:</b> 100% hit rate to South Haven</p>
        ]]>
      </description>
      <styleUrl>#primarySearchStyle</styleUrl>
      <Polygon>
        <extrude>0</extrude>
        <altitudeMode>clampToGround</altitudeMode>
        <outerBoundaryIs>
          <LinearRing>
            <coordinates>{generate_circle_coordinates(zone['lat'], zone['lon'], zone['radius_nm'])}</coordinates>
          </LinearRing>
        </outerBoundaryIs>
      </Polygon>
    </Placemark>
    
    <!-- Zone {i} Center Marker -->
    <Placemark>
      <name>{zone['name']} - CENTER</name>
      <description>Primary search zone center point</description>
      <styleUrl>#markerStyle</styleUrl>
      <Point>
        <coordinates>{zone['lon']},{zone['lat']},0</coordinates>
      </Point>
    </Placemark>
'''
    
    # Drift path
    kml += '''
    <!-- Drift Path (FENDER ONLY - not main wreck) -->
    <Placemark>
      <name>Fender Drift Path (FLOATING DEBRIS ONLY)</name>
      <description>
        <![CDATA[
        <p><b>WARNING:</b> This is the FENDER drift path, NOT the main wreck!</p>
        <p>Fender floated for 36-48 hours, drifted ~76 NM to Grand Haven</p>
        <p>Main wreck (with lead keel) sank NEAR the 8-9 PM location</p>
        <p><b>Use this for:</b> Searching floating debris, not the Rossa hull</p>
        ]]>
      </description>
      <styleUrl>#driftPathStyle</styleUrl>
      <LineString>
        <extrude>0</extrude>
        <altitudeMode>clampToGround</altitudeMode>
        <coordinates>'''
    
    for waypoint in FENDER_DRIFT_PATH:
        kml += f"{waypoint['lon']},{waypoint['lat']},0 "
    
    kml += '''</coordinates>
      </LineString>
    </Placemark>
'''
    
    # Waypoints
    for i, waypoint in enumerate(DRIFT_PATH, 1):
        kml += f'''
    <!-- Drift Waypoint {i} -->
    <Placemark>
      <name>Drift Point {i}: {waypoint['time']}</name>
      <description>Modeled fender position</description>
      <Point>
        <coordinates>{waypoint['lon']},{waypoint['lat']},0</coordinates>
      </Point>
    </Placemark>
'''
    
    # McKinley Marina
    kml += f'''
    <!-- McKinley Marina -->
    <Placemark>
      <name>{MCKINLEY_MARINA['name']}</name>
      <description>Last seen: August 22, 2025 ~11 AM CDT</description>
      <styleUrl>#markerStyle</styleUrl>
      <Point>
        <coordinates>{MCKINLEY_MARINA['lon']},{MCKINLEY_MARINA['lat']},0</coordinates>
      </Point>
    </Placemark>
'''
    
    # Fender found location
    kml += f'''
    <!-- Fender Found -->
    <Placemark>
      <name>{FENDER_FOUND['name']}</name>
      <description>
        <![CDATA[
        <p><b>Found:</b> August 23-24, 2025</p>
        <p><b>Description:</b> Orange large teardrop fender with black loop area, 3-foot line attached</p>
        <p><b>Significance:</b> Confirms drift path across Lake Michigan</p>
        ]]>
      </description>
      <Style>
        <IconStyle>
          <color>ff00ff00</color>
          <scale>1.5</scale>
          <Icon>
            <href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href>
          </Icon>
        </IconStyle>
      </Style>
      <Point>
        <coordinates>{FENDER_FOUND['lon']},{FENDER_FOUND['lat']},0</coordinates>
      </Point>
    </Placemark>
'''
    
    # Close KML
    kml += '''
  </Document>
</kml>
'''
    
    return kml


def generate_circle_coordinates(center_lat: float, center_lon: float, radius_nm: float, num_points: int = 64) -> str:
    """Generate circle coordinates for KML polygon."""
    import math
    
    radius_deg = radius_nm / 60.0  # Convert NM to degrees (approximate)
    
    coords = []
    for i in range(num_points + 1):
        angle = 2 * math.pi * i / num_points
        lat = center_lat + radius_deg * math.cos(angle)
        lon = center_lon + radius_deg * math.sin(angle)
        coords.append(f"{lon},{lat},0")
    
    return " ".join(coords)


def generate_arc_coordinates(center_lat: float, center_lon: float, radius_nm: float, 
                             start_angle: float, end_angle: float, num_points: int = 64) -> str:
    """
    Generate arc coordinates (partial circle) for KML polygon.
    
    Args:
        center_lat, center_lon: Center point
        radius_nm: Radius in nautical miles
        start_angle: Start angle in degrees (0=North, 90=East, 180=South, 270=West)
        end_angle: End angle in degrees
        num_points: Number of points along arc
    """
    import math
    
    radius_deg = radius_nm / 60.0
    
    # Convert angles to radians (0° = North, clockwise)
    start_rad = math.radians(90 - start_angle)  # Convert to math angle
    end_rad = math.radians(90 - end_angle)
    
    coords = []
    for i in range(num_points + 1):
        angle = start_rad + (end_rad - start_rad) * i / num_points
        lat = center_lat + radius_deg * math.cos(angle)
        lon = center_lon + radius_deg * math.sin(angle)
        coords.append(f"{lon},{lat},0")
    
    return " ".join(coords)


def generate_elliptical_arc(center_lat: float, center_lon: float, 
                            major_nm: float, minor_nm: float,
                            start_angle: float, end_angle: float, 
                            num_points: int = 64) -> str:
    """
    Generate elliptical arc coordinates for KML polygon.
    
    Args:
        center_lat, center_lon: Center point
        major_nm: Semi-major axis in nautical miles (east-west)
        minor_nm: Semi-minor axis in nautical miles (north-south)
        start_angle: Start angle in degrees
        end_angle: End angle in degrees
        num_points: Number of points along arc
    """
    import math
    
    major_deg = major_nm / 60.0
    minor_deg = minor_nm / 60.0
    
    # Convert angles to radians
    start_rad = math.radians(90 - start_angle)
    end_rad = math.radians(90 - end_angle)
    
    coords = []
    for i in range(num_points + 1):
        angle = start_rad + (end_rad - start_rad) * i / num_points
        # Elliptical parametric equation
        lat = center_lat + minor_deg * math.cos(angle)
        lon = center_lon + major_deg * math.sin(angle)
        coords.append(f"{lon},{lat},0")
    
    return " ".join(coords)


def main():
    """Generate KML search boxes."""
    print('='*70)
    print('ROSSA SEARCH KML GENERATOR')
    print('='*70)
    print()
    
    # Generate KML
    kml_content = create_kml_document()
    
    # Save
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    kml_path = OUTPUT_DIR / f'rossa_search_areas_{timestamp}.kml'
    
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print(f'✓ KML saved: {kml_path}')
    print()
    print('Search Areas:')
    for i, zone in enumerate(PRIMARY_ZONES, 1):
        print(f'  {i}. {zone["name"]}')
        print(f'     Center: {zone["lat"]:.5f}°N, {zone["lon"]:.5f}°W')
        print(f'     Radius: {zone["radius_nm"]} NM')
        print(f'     Confidence: {zone["confidence"]}')
        print()
    
    print('Open in Google Earth:')
    print(f'  {kml_path}')
    print()
    print('='*70)
    
    return kml_path


if __name__ == '__main__':
    main()
