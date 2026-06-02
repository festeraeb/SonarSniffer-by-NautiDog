"""
TOP 10 LAKE MICHIGAN TARGETS - KML GENERATOR

Creates KML/KMZ with all 10 targets from multi-sensor analysis:
- All 10 targets with rankings, scores, pop-ups
- Andaste coordinates highlighted (Targets #1 and #4)
- Color-coded by confidence level (5-star = red, 4-star = yellow, 3-star = green)
- SWOT analysis status for each target
"""

import json
from pathlib import Path
from datetime import datetime

# ── TOP 10 TARGETS FROM MULTI-SENSOR ANALYSIS ────────────────────────────────

TOP_10_TARGETS = [
    {
        'rank': 1,
        'name': 'The Blue Anomaly (Andaste Main Hull)',
        'lat': 42.4729,
        'lon': -87.0970,
        'depth_m': 150,
        'confidence_stars': 5,
        'sensor_hits': {
            'B02_Blue': {'score': 1.14, 'status': 'STRONG'},
            'B04_Red': {'score': 1.23, 'status': 'STRONGEST'},
            'B01_Coastal': {'score': 0.48, 'status': 'Moderate'},
        },
        'why_promising': 'Blue band penetration suggests submerged object. Red band signature indicates solid structure. Located in Zion/Waukegan trench.',
        'classification': 'HEAVY_STEEL_MASS',
        'status': '✅ THERMAL CONFIRMED (Andaste Main Hull)',
    },
    {
        'rank': 2,
        'name': 'The Red Edge',
        'lat': 42.4656,
        'lon': -87.0856,
        'depth_m': 120,
        'confidence_stars': 5,
        'sensor_hits': {
            'B02_Blue': {'score': 1.06, 'status': 'Strong'},
            'B04_Red': {'score': 1.14, 'status': 'Very Strong'},
            'B01_Coastal': {'score': 0.52, 'status': 'Moderate'},
        },
        'why_promising': 'Consistent multi-band signature. Near known shipping lane. Size matches Bristol 32 class.',
        'classification': 'POSSIBLE_VESSEL',
        'status': '⏳ PENDING VERIFICATION',
    },
    {
        'rank': 3,
        'name': 'The Trench Shadow',
        'lat': 42.4623,
        'lon': -87.1026,
        'depth_m': 160,
        'confidence_stars': 4,
        'sensor_hits': {
            'B02_Blue': {'score': 0.99, 'status': 'Good'},
            'B04_Red': {'score': 1.02, 'status': 'Strong'},
            'B01_Coastal': {'score': 0.0, 'status': 'Weak'},
        },
        'why_promising': 'Deepest target (well-preserved). Cold water = slow decomposition. Possible Andaste candidate.',
        'classification': 'HISTORICAL_WRECK_CANDIDATE',
        'status': '⏳ PENDING THERMAL',
    },
    {
        'rank': 4,
        'name': 'The Northern Blip (Andaste Broken Section)',
        'lat': 42.4675,
        'lon': -87.0813,
        'depth_m': 140,
        'confidence_stars': 4,
        'sensor_hits': {
            'B02_Blue': {'score': 0.94, 'status': 'Good'},
            'B04_Red': {'score': 0.91, 'status': 'Good'},
            'B01_Coastal': {'score': 0.50, 'status': 'Moderate'},
        },
        'why_promising': 'Elongated signature (intact hull). Orientation NE-SW. Possible intact vessel.',
        'classification': 'HEAVY_STEEL_MASS',
        'status': '✅ THERMAL CONFIRMED (Andaste Broken Section)',
    },
    {
        'rank': 5,
        'name': 'The Western Edge',
        'lat': 42.4645,
        'lon': -87.1013,
        'depth_m': 155,
        'confidence_stars': 4,
        'sensor_hits': {
            'B02_Blue': {'score': 0.95, 'status': 'Good'},
            'B04_Red': {'score': 0.88, 'status': 'Good'},
            'B01_Coastal': {'score': 0.0, 'status': 'Weak'},
        },
        'why_promising': 'Near western edge of trench. Possible grounding candidate. Size ~20m.',
        'classification': 'COMMERCIAL_VESSEL_CANDIDATE',
        'status': '⏳ PENDING THERMAL',
    },
    {
        'rank': 6,
        'name': 'The Deep Anomaly',
        'lat': 42.4551,
        'lon': -87.1100,
        'depth_m': 175,
        'confidence_stars': 3,
        'sensor_hits': {
            'B02_Blue': {'score': 0.88, 'status': 'Fair'},
            'B04_Red': {'score': 0.85, 'status': 'Fair'},
            'B01_Coastal': {'score': 0.0, 'status': 'No hit'},
        },
        'why_promising': 'Deepest target (excellent preservation). Possible pre-1900 wooden wreck.',
        'classification': 'WOODEN_WRECK_CANDIDATE',
        'status': '⏳ PENDING VERIFICATION',
    },
    {
        'rank': 7,
        'name': 'The Central Cluster',
        'lat': 42.4606,
        'lon': -87.1018,
        'depth_m': 145,
        'confidence_stars': 4,
        'sensor_hits': {
            'B02_Blue': {'score': 0.94, 'status': 'Good'},
            'B04_Red': {'score': 0.91, 'status': 'Good'},
            'B01_Coastal': {'score': 0.48, 'status': 'Moderate'},
        },
        'why_promising': 'Part of cluster (3 targets within 500m). Possible collision site.',
        'classification': 'MULTIPLE_VESSELS',
        'status': '⏳ PENDING THERMAL',
    },
    {
        'rank': 8,
        'name': 'The Eastern Rise',
        'lat': 42.4678,
        'lon': -87.0913,
        'depth_m': 130,
        'confidence_stars': 3,
        'sensor_hits': {
            'B02_Blue': {'score': 0.88, 'status': 'Fair'},
            'B04_Red': {'score': 0.85, 'status': 'Fair'},
            'B01_Coastal': {'score': 0.0, 'status': 'Weak'},
        },
        'why_promising': 'Elevated position (visible to sonar). Possible navigation hazard.',
        'classification': 'NAVIGATION_HAZARD',
        'status': '⏳ PENDING VERIFICATION',
    },
    {
        'rank': 9,
        'name': 'The Southern Shadow',
        'lat': 42.4586,
        'lon': -87.1059,
        'depth_m': 165,
        'confidence_stars': 3,
        'sensor_hits': {
            'B02_Blue': {'score': 0.85, 'status': 'Fair'},
            'B04_Red': {'score': 0.82, 'status': 'Fair'},
            'B01_Coastal': {'score': 0.0, 'status': 'No hit'},
        },
        'why_promising': 'Southern trench location. Possible cargo vessel (larger signature).',
        'classification': 'CARGO_VESSEL_CANDIDATE',
        'status': '⏳ PENDING VERIFICATION',
    },
    {
        'rank': 10,
        'name': 'The Mystery Blip',
        'lat': 42.4628,
        'lon': -87.0759,
        'depth_m': 110,
        'confidence_stars': 3,
        'sensor_hits': {
            'B02_Blue': {'score': 0.85, 'status': 'Fair'},
            'B04_Red': {'score': 0.81, 'status': 'Fair'},
            'B01_Coastal': {'score': 0.0, 'status': 'Weak'},
        },
        'why_promising': 'Shallowest target (easy dive access). Possible modern wreck (post-1950).',
        'classification': 'MODERN_WRECK_CANDIDATE',
        'status': '⏳ PENDING VERIFICATION',
    },
]

# ── KML GENERATION ────────────────────────────────────────────────────────────

def get_color(confidence_stars):
    """Get color based on confidence level."""
    if confidence_stars >= 5:
        return 'ff0000ff'  # Red (5-star)
    elif confidence_stars >= 4:
        return 'ff00ffff'  # Yellow (4-star)
    else:
        return 'ff00ff00'  # Green (3-star)

def create_kml():
    """Create KML content with all 10 targets."""
    
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>Top 10 Lake Michigan Targets - Multi-Sensor Analysis</name>
    <description>
      <![CDATA[
      <h1>WreckHunter2000 - Top 10 Lake Michigan Targets</h1>
      <p><b>Analysis Date:</b> March 24, 2026</p>
      <p><b>Scene:</b> S2C_16TDN_20250916_0_L2A (Sentinel-2, Sept 16, 2025)</p>
      <p><b>Processing:</b> GPU Chunked (Quadro M2200)</p>
      <p><b>Analyst:</b> Thom Hadfield (NautiDog)</p>
      <p><b>Method:</b> Multi-sensor spectral stacking (B01, B02, B04 bands)</p>
      <p><b>Color Coding:</b></p>
      <ul>
        <li><span style="color: red;">RED</span> = 5-star confidence (Highest priority)</li>
        <li><span style="color: yellow;">YELLOW</span> = 4-star confidence (High priority)</li>
        <li><span style="color: green;">GREEN</span> = 3-star confidence (Moderate priority)</li>
      </ul>
      <p><b>Special Notes:</b></p>
      <ul>
        <li>Targets #1 and #4 are thermally confirmed as Andaste whaleback (1907)</li>
        <li>SWOT analysis pending for all targets</li>
        <li>Dedicated to Dennis Hadfield (March 2025)</li>
      </ul>
      <p><i>"The lake doesn't give up her dead, but she can no longer hide their bones from the math."</i></p>
      ]]>
    </description>
    
    <!-- Styles -->
    <Style id="five_star">
      <IconStyle>
        <color>ff0000ff</color>
        <scale>1.5</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
    
    <Style id="four_star">
      <IconStyle>
        <color>ff00ffff</color>
        <scale>1.2</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
    
    <Style id="three_star">
      <IconStyle>
        <color>ff00ff00</color>
        <scale>1.0</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
'''
    
    # Add all 10 targets
    kml += '\n    <!-- TOP 10 TARGETS -->\n'
    
    for target in TOP_10_TARGETS:
        color_style = get_color(target['confidence_stars'])
        
        # Build sensor hits table
        sensor_table = ''
        for band, data in target['sensor_hits'].items():
            sensor_table += f'<tr><td>{band}:</td><td>{data["score"]} ({data["status"]})</td></tr>'
        
        kml += f'''
    <Placemark>
      <name>#{target["rank"]} - {target["name"][:40]} ({target["confidence_stars"]}⭐)</name>
      <description>
        <![CDATA[
        <h2>Target #{target["rank"]}: {target["name"]}</h2>
        <table>
          <tr><td><b>Rank:</b></td><td>#{target["rank"]}</td></tr>
          <tr><td><b>Confidence:</b></td><td>{"⭐" * target["confidence_stars"]}</td></tr>
          <tr><td><b>Coordinates:</b></td><td>{target["lat"]:.6f}°N, {target["lon"]:.6f}°W</td></tr>
          <tr><td><b>Depth:</b></td><td>~{target["depth_m"]}m</td></tr>
          <tr><td><b>Classification:</b></td><td>{target["classification"]}</td></tr>
          <tr><td><b>Status:</b></td><td>{target["status"]}</td></tr>
          <tr><td colspan="2"><b>Sensor Hits:</b></td></tr>
          {sensor_table}
          <tr><td><b>Why Promising:</b></td><td>{target["why_promising"]}</td></tr>
        </table>
        <p><i>Generated by WreckHunter2000 - CESARops</i></p>
        <p><i>Dedicated to Dennis Hadfield (March 2025)</i></p>
        ]]>
      </description>
      <styleUrl>#{color_style}</styleUrl>
      <Point>
        <coordinates>{target['lon']},{target['lat']},0</coordinates>
      </Point>
    </Placemark>
'''
    
    # Close KML
    kml += '''
  </Document>
</kml>
'''
    
    return kml

# ── MAIN ──────────────────────────────────────────────────────────────────────

if __name__ == '__main__':
    print('='*80)
    print('TOP 10 LAKE MICHIGAN TARGETS - KML GENERATOR')
    print('='*80)
    print()
    
    # Generate KML
    kml_content = create_kml()
    
    # Save KML
    kml_path = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/top_10_lake_michigan_targets.kml')
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print(f'KML saved: {kml_path}')
    print()
    print('TOP 10 TARGETS:')
    for target in TOP_10_TARGETS:
        andaste_note = ' (Andaste)' if target['rank'] in [1, 4] else ''
        print(f'  #{target["rank"]}: {target["name"][:40]}{andaste_note} - {target["confidence_stars"]}⭐ at {target["lat"]:.6f}N, {target["lon"]:.6f}W')
    print()
    print('Open in Google Earth to view with pop-ups and color coding.')
    print('='*80)
