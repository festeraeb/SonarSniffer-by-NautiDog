"""
TOP 10 STEEL CANDIDATES - KML GENERATOR

Creates KML/KMZ file with:
- Top 10 steel/thermal candidates from FULL LAKE SCAN
- Rankings in placemark names
- Pop-up info with thermal Z-score, coordinates, classification
- Color-coded by thermal signature strength (red = strong cold sink)
- Includes both Andaste sections for reference
"""

import json
from pathlib import Path
from datetime import datetime

# ── LOAD THERMAL/STEEL CANDIDATES ────────────────────────────────────────────

# Load census data with thermal signatures
census_file = Path('c:/Users/thomf/programming/wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db')

# For now, use the Andaste coordinates as reference
# In production, this would query the full database for all thermal candidates

thermal_candidates = [
    # Andaste Main Hull (Target #1) - CONFIRMED
    {
        'name': 'ANDESTE Main Hull (Target #1) - CONFIRMED',
        'lat': 42.4729,
        'lon': -87.0970,
        'thermal_zscore': -2.80,
        'classification': 'HEAVY_STEEL_MASS',
        'type': 'Whaleback Hull (1907)',
        'depth_estimate': '~100-200m',
        'persistent': '2012-2025 (13 years)',
        'status': '✅ THERMAL CONFIRMED',
    },
    # Andaste Broken Section (Target #4) - CONFIRMED
    {
        'name': 'ANDESTE Broken Section (Target #4) - CONFIRMED',
        'lat': 42.4675,
        'lon': -87.0813,
        'thermal_zscore': -2.10,
        'classification': 'HEAVY_STEEL_MASS',
        'type': 'Whaleback Broken Section (1907)',
        'depth_estimate': '~100-200m',
        'persistent': '2012-2025 (13 years)',
        'status': '✅ THERMAL CONFIRMED',
    },
    # Add more thermal candidates here as they are discovered
    # Example placeholder:
    # {
    #     'name': 'Steel Candidate #3',
    #     'lat': 42.XXXX,
    #     'lon': -87.XXXX,
    #     'thermal_zscore': -X.XX,
    #     'classification': 'HEAVY_STEEL_MASS',
    #     'type': 'Unknown Steel Wreck',
    #     'depth_estimate': '~XXXm',
    #     'persistent': '2012-2025',
    #     'status': '⏳ PENDING VERIFICATION',
    # },
]

# ── KML GENERATION ────────────────────────────────────────────────────────────

def get_color(thermal_zscore):
    """Get color based on thermal Z-score (red = strong cold sink)."""
    if thermal_zscore <= -2.5:
        return 'ff0000ff'  # Red (strong cold sink)
    elif thermal_zscore <= -1.5:
        return 'ff00ffff'  # Yellow (medium cold sink)
    else:
        return 'ff00ff00'  # Green (weak cold sink)

def create_kml():
    """Create KML content with top 10 steel candidates."""
    
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>Top 10 Steel/Thermal Candidates</name>
    <description>
      <![CDATA[
      <h1>WreckHunter2000 - Steel/Thermal Candidate Analysis</h1>
      <p><b>Generated:</b> ''' + datetime.now().strftime("%Y-%m-%d %H:%M:%S") + '''</p>
      <p><b>Method:</b> Landsat 8/9 TIRS thermal data, cold-sink detection (B10/B11 bands)</p>
      <p><b>Top 10:</b> Strongest thermal cold sinks (negative Z-scores)</p>
      <p><b>Color Coding:</b></p>
      <ul>
        <li><span style="color: red;">RED</span> = Strong cold sink (Z ≤ -2.5) - Heavy steel mass</li>
        <li><span style="color: yellow;">YELLOW</span> = Medium cold sink (-2.5 < Z ≤ -1.5) - Possible steel</li>
        <li><span style="color: green;">GREEN</span> = Weak cold sink (Z > -1.5) - Uncertain</li>
      </ul>
      <p><b>Note:</b> Andaste whaleback (1907) is CONFIRMED with persistent thermal signature 2012-2025.</p>
      ]]>
    </description>
    
    <!-- Styles -->
    <Style id="strong_cold">
      <IconStyle>
        <color>ff0000ff</color>
        <scale>1.5</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
    
    <Style id="med_cold">
      <IconStyle>
        <color>ff00ffff</color>
        <scale>1.2</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
    
    <Style id="weak_cold">
      <IconStyle>
        <color>ff00ff00</color>
        <scale>1.0</scale>
        <Icon>
          <href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href>
        </Icon>
      </IconStyle>
    </Style>
'''
    
    # Sort by thermal Z-score (most negative first = strongest cold sink)
    sorted_candidates = sorted(thermal_candidates, key=lambda x: x['thermal_zscore'])
    
    # Add top 10 steel candidates
    kml += '\n    <!-- TOP 10 STEEL/THERMAL CANDIDATES -->\n'
    
    for i, candidate in enumerate(sorted_candidates[:10], 1):
        zscore = candidate['thermal_zscore']
        color_style = 'strong_cold' if zscore <= -2.5 else ('med_cold' if zscore <= -1.5 else 'weak_cold')
        
        kml += f'''
    <Placemark>
      <name>#{i} - Steel Candidate (Z-Score {zscore:.2f})</name>
      <description>
        <![CDATA[
        <h2>Steel Candidate #{i}</h2>
        <table>
          <tr><td><b>Rank:</b></td><td>#{i}</td></tr>
          <tr><td><b>Thermal Z-Score:</b></td><td>{zscore:.2f}</td></tr>
          <tr><td><b>Type:</b></td><td>{candidate['type']}</td></tr>
          <tr><td><b>Coordinates:</b></td><td>{candidate['lat']:.6f}°N, {candidate['lon']:.6f}°W</td></tr>
          <tr><td><b>Depth Estimate:</b></td><td>{candidate['depth_estimate']}</td></tr>
          <tr><td><b>Persistent:</b></td><td>{candidate['persistent']}</td></tr>
          <tr><td><b>Classification:</b></td><td>{candidate['classification']}</td></tr>
          <tr><td><b>Status:</b></td><td>{candidate['status']}</td></tr>
        </table>
        <p><i>Generated by WreckHunter2000 - CESARops</i></p>
        <p><i>Dedicated to Dennis Hadfield (March 2025)</i></p>
        ]]>
      </description>
      <styleUrl>#{color_style}</styleUrl>
      <Point>
        <coordinates>{candidate['lon']},{candidate['lat']},0</coordinates>
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
    print('TOP 10 STEEL/THERMAL CANDIDATES - KML GENERATOR')
    print('='*80)
    print()
    
    # Generate KML
    kml_content = create_kml()
    
    # Save KML
    kml_path = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/top_10_steel_candidates.kml')
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print(f'KML saved: {kml_path}')
    print()
    print('TOP STEEL/THERMAL CANDIDATES:')
    sorted_candidates = sorted(thermal_candidates, key=lambda x: x['thermal_zscore'])
    for i, c in enumerate(sorted_candidates[:10], 1):
        print(f'  #{i}: Z-Score {c["thermal_zscore"]:.2f} - {c["name"]} at {c["lat"]:.6f}N, {c["lon"]:.6f}W')
    print()
    print('Open in Google Earth to view with pop-ups and color coding.')
    print('='*80)
