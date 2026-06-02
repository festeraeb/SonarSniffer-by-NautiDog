"""
TOP 10 STEEL CANDIDATES FROM FULL CENSUS - KML GENERATOR

Queries LAKE_MICHIGAN_CENSUS_2026.db for ALL thermal candidates,
ranks by thermal Z-score, and creates KML with top 10.
"""

import sqlite3
import json
from pathlib import Path
from datetime import datetime

# ── LOAD THERMAL CANDIDATES FROM DATABASE ────────────────────────────────────

db_path = Path('c:/Users/thomf/programming/wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db')

if not db_path.exists():
    print(f'ERROR: Database not found at {db_path}')
    print('Run lake_census_engine.py first to create database.')
    exit(1)

conn = sqlite3.connect(str(db_path))
cur = conn.cursor()

# Query all anomaly hits with thermal data
cur.execute("""
    SELECT lat, lon, concept, score, thermal_sink_l8, metric_zscore
    FROM anomaly_hits
    WHERE thermal_sink_l8 = 1 OR metric_zscore < -1.5
    ORDER BY metric_zscore ASC
    LIMIT 50
""")

thermal_candidates = []
for row in cur.fetchall():
    lat, lon, concept, score, thermal_sink, zscore = row
    thermal_candidates.append({
        'name': f'{concept or "Unknown"} (Score: {score})',
        'lat': lat,
        'lon': lon,
        'thermal_zscore': zscore if zscore else -999,
        'classification': 'HEAVY_STEEL_MASS' if thermal_sink else 'POSSIBLE_STEEL',
        'type': 'Steel Wreck Candidate',
        'depth_estimate': '~Unknown (need bathymetry)',
        'persistent': 'Single epoch detection',
        'status': '⏳ PENDING VERIFICATION',
    })

conn.close()

# Add Andaste coordinates (confirmed)
thermal_candidates.extend([
    {
        'name': 'ANDESTE Main Hull (Target #1) - ✅ CONFIRMED',
        'lat': 42.4729,
        'lon': -87.0970,
        'thermal_zscore': -2.80,
        'classification': 'HEAVY_STEEL_MASS',
        'type': 'Whaleback Hull (1907)',
        'depth_estimate': '~100-200m',
        'persistent': '2012-2025 (13 years)',
        'status': '✅ THERMAL CONFIRMED',
    },
    {
        'name': 'ANDESTE Broken Section (Target #4) - ✅ CONFIRMED',
        'lat': 42.4675,
        'lon': -87.0813,
        'thermal_zscore': -2.10,
        'classification': 'HEAVY_STEEL_MASS',
        'type': 'Whaleback Broken Section (1907)',
        'depth_estimate': '~100-200m',
        'persistent': '2012-2025 (13 years)',
        'status': '✅ THERMAL CONFIRMED',
    },
])

# Sort by thermal Z-score (most negative = strongest cold sink)
sorted_candidates = sorted(thermal_candidates, key=lambda x: x['thermal_zscore'])
top_10 = sorted_candidates[:10]

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
    <name>Top 10 Steel/Thermal Candidates - Full Census</name>
    <description>
      <![CDATA[
      <h1>WreckHunter2000 - Steel/Thermal Candidate Analysis</h1>
      <p><b>Generated:</b> ''' + datetime.now().strftime("%Y-%m-%d %H:%M:%S") + '''</p>
      <p><b>Source:</b> LAKE_MICHIGAN_CENSUS_2026.db (Full Lake Scan)</p>
      <p><b>Method:</b> Landsat 8/9 TIRS thermal data, cold-sink detection</p>
      <p><b>Top 10:</b> Strongest thermal cold sinks (most negative Z-scores)</p>
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
    
    # Add top 10 steel candidates
    kml += '\n    <!-- TOP 10 STEEL/THERMAL CANDIDATES -->\n'
    
    for i, candidate in enumerate(top_10, 1):
        zscore = candidate['thermal_zscore']
        color_style = 'strong_cold' if zscore <= -2.5 else ('med_cold' if zscore <= -1.5 else 'weak_cold')
        
        kml += f'''
    <Placemark>
      <name>#{i} - {candidate["name"][:50]} (Z={zscore:.2f})</name>
      <description>
        <![CDATA[
        <h2>Steel Candidate #{i}</h2>
        <table>
          <tr><td><b>Rank:</b></td><td>#{i}</td></tr>
          <tr><td><b>Thermal Z-Score:</b></td><td>{zscore:.2f}</td></tr>
          <tr><td><b>Type:</b></td><td>{candidate["type"]}</td></tr>
          <tr><td><b>Coordinates:</b></td><td>{candidate["lat"]:.6f}°N, {candidate["lon"]:.6f}°W</td></tr>
          <tr><td><b>Depth Estimate:</b></td><td>{candidate["depth_estimate"]}</td></tr>
          <tr><td><b>Persistent:</b></td><td>{candidate["persistent"]}</td></tr>
          <tr><td><b>Classification:</b></td><td>{candidate["classification"]}</td></tr>
          <tr><td><b>Status:</b></td><td>{candidate["status"]}</td></tr>
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
    print('TOP 10 STEEL CANDIDATES FROM FULL CENSUS - KML GENERATOR')
    print('='*80)
    print()
    
    print(f'Loaded {len(thermal_candidates)} thermal candidates from database')
    print()
    
    # Generate KML
    kml_content = create_kml()
    
    # Save KML
    kml_path = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/top_10_steel_full_census.kml')
    with open(kml_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print(f'KML saved: {kml_path}')
    print()
    print('TOP 10 STEEL/THERMAL CANDIDATES (by Z-score):')
    for i, c in enumerate(top_10, 1):
        print(f'  #{i}: Z-Score {c["thermal_zscore"]:.2f} - {c["name"][:50]} at {c["lat"]:.6f}N, {c["lon"]:.6f}W')
    print()
    print('Open in Google Earth to view with pop-ups and color coding.')
    print('='*80)
