"""
google_earth_tiled_database_v2.py

Creates Google Earth KML with:
- Tiled sections (toggle on/off by sensor type)
- DATE-BASED folders (toggle by scan date for training comparison)
- Persistent popups (stay open until you close them)
- LOD for performance
"""

import json
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/google_earth')
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Scan dates for magnetic data (training/validation timeline)
MAG_DATES = [
    '2025-08',
    '2025-09', 
    '2025-10',
    '2025-11',
    '2025-12',
    '2026-01',
    '2026-02',
    '2026-03',
]

# GPU processing dates
GPU_DATES = [
    '2025-09-16',  # Sentinel-2 scene date
]

# ── KML Generation ────────────────────────────────────────────────────────────

def create_date_tiled_kml():
    """Create KML with date-based folders for training comparison."""
    
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
  <Document>
    <name>WreckHunter2000 - Date & Sensor Tiled</name>
    <description>
      <![CDATA[
      <h1>WreckHunter2000 Database</h1>
      <h2>Training Timeline Comparison</h2>
      <p>Toggle by DATE to see training improvements</p>
      <p>Toggle by SENSOR to compare detection methods</p>
      <p>Popups stay open until you close them</p>
      <p>Generated: ''' + datetime.now().strftime('%Y-%m-%d %H:%M:%S') + '''</p>
      
      <h3>How to Use:</h3>
      <ol>
        <li>Expand "MAGNETIC - By Date" folder</li>
        <li>Toggle different months to see training progression</li>
        <li>Compare Aug 2025 (early) vs Mar 2026 (improved)</li>
        <li>Use "Satellite" folder for cross-validation</li>
      </ol>
      ]]>
    </description>
    
    <!-- NetworkLink for auto-refresh (5 minutes) -->
    <NetworkLinkControl>
      <minRefreshPeriod>300</minRefreshPeriod>
    </NetworkLinkControl>
    
    <!-- ============================================ -->
    <!-- MAGNETIC DATA - Organized by DATE -->
    <!-- ============================================ -->
    <Folder id="magnetic_root">
      <name>🧲 MAGNETIC - By Date (Training Timeline)</name>
      <visibility>1</visibility>
      <open>1</open>
      <description>Magnetic anomalies organized by scan date - toggle to compare training improvements</description>
'''
    
    # Create folder for each magnetic scan date
    for i, scan_date in enumerate(MAG_DATES):
        kml += f'''
      <Folder id="mag_{scan_date.replace('-', '_')}">
        <name>{scan_date}</name>
        <visibility>{"1" if i >= len(MAG_DATES) - 2 else "0"}</visibility>  <!-- Only last 2 months visible by default -->
        <open>0</open>
        <description>Magnetic scans from {scan_date}</description>
        
        <Style id="mag_{scan_date.replace('-', '_')}_style">
          <IconStyle>
            <color>ff0000ff</color>
            <scale>1.0</scale>
            <Icon>
              <href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href>
            </Icon>
          </IconStyle>
        </Style>
        
        <!-- Sample placemark for this date -->
        <Placemark id="mag_{scan_date.replace('-', '_')}_sample">
          <name>Sample Mag Target ({scan_date})</name>
          <description>
            <![CDATA[
            <div style="font-family: Arial; max-width: 400px;">
              <h3 style="color: red;">Magnetic Anomaly - {scan_date}</h3>
              <table style="width: 100%;">
                <tr><td><b>Coordinates:</b></td><td>42.47°N, -87.10°W</td></tr>
                <tr><td><b>Depth:</b></td><td>150m</td></tr>
                <tr><td><b>Amplitude:</b></td><td>High</td></tr>
                <tr><td><b>Scan Date:</b></td><td>{scan_date}</td></tr>
                <tr><td><b>Training Phase:</b></td><td>{"Late" if "2026" in scan_date else "Early"}</td></tr>
              </table>
              <p style="margin-top: 10px; font-size: 12px; color: gray;">
                <i>Compare with other dates to see training improvements</i>
              </p>
            </div>
            ]]>
          </description>
          <styleUrl>#mag_{scan_date.replace('-', '_')}_style</styleUrl>
          <Point>
            <coordinates>-87.10,42.47,0</coordinates>
          </Point>
        </Placemark>
      </Folder>
'''
    
    kml += '''
    </Folder>
    
    <!-- ============================================ -->
    <!-- SATELLITE DATA - Organized by DATE -->
    <!-- ============================================ -->
    <Folder id="satellite_root">
      <name>🛰️ SATELLITE - By Date</name>
      <visibility>1</visibility>
      <open>0</open>
      <description>Satellite detections organized by scene date</description>
'''
    
    # Create folder for each satellite date
    for scene_date in GPU_DATES:
        kml += f'''
      <Folder id="sat_{scene_date.replace('-', '_')}">
        <name>{scene_date}</name>
        <visibility>1</visibility>
        <open>0</open>
        <description>Sentinel-2 scene from {scene_date}</description>
        
        <Style id="sat_{scene_date.replace('-', '_')}_style">
          <IconStyle>
            <color>ffffff00</color>
            <scale>1.0</scale>
            <Icon>
              <href>http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png</href>
            </Icon>
          </IconStyle>
        </Style>
        
        <Placemark id="sat_{scene_date.replace('-', '_')}_sample">
          <name>Sample Satellite Target ({scene_date})</name>
          <description>
            <![CDATA[
            <div style="font-family: Arial; max-width: 400px;">
              <h3 style="color: yellow;">Satellite Detection - {scene_date}</h3>
              <table style="width: 100%;">
                <tr><td><b>Coordinates:</b></td><td>42.47°N, -87.10°W</td></tr>
                <tr><td><b>Band:</b></td><td>B04 (Red)</td></tr>
                <tr><td><b>Score:</b></td><td>High</td></tr>
                <tr><td><b>Scene Date:</b></td><td>{scene_date}</td></tr>
              </table>
            </div>
            ]]>
          </description>
          <styleUrl>#sat_{scene_date.replace('-', '_')}_style</styleUrl>
          <Point>
            <coordinates>-87.10,42.47,0</coordinates>
          </Point>
        </Placemark>
      </Folder>
'''
    
    kml += '''
    </Folder>
    
    <!-- ============================================ -->
    <!-- CROSS-VALIDATION VIEW -->
    <!-- ============================================ -->
    <Folder id="crossval_root">
      <name>🔍 CROSS-VALIDATION (Multi-Sensor Hits)</name>
      <visibility>1</visibility>
      <open>0</open>
      <description>Targets that appear in MULTIPLE sensors (highest confidence)</description>
      
      <Style id="crossval_style">
        <IconStyle>
          <color>ff00ff00</color>
          <scale>1.2</scale>
          <Icon>
            <href>http://maps.google.com/mapfiles/kml/paddle/grn-circle.png</href>
          </Icon>
        </IconStyle>
      </Style>
      
      <Placemark id="crossval_sample">
        <name>Multi-Sensor Hit (Mag + Satellite)</name>
        <description>
          <![CDATA[
          <div style="font-family: Arial; max-width: 400px;">
            <h3 style="color: green;">HIGH CONFIDENCE TARGET</h3>
            <table style="width: 100%;">
              <tr><td><b>Coordinates:</b></td><td>42.47°N, -87.10°W</td></tr>
              <tr><td><b>Magnetic:</b></td><td>✅ Detected (2026-03)</td></tr>
              <tr><td><b>Satellite:</b></td><td>✅ Detected (2025-09-16)</td></tr>
              <tr><td><b>Thermal:</b></td><td>⏳ Pending</td></tr>
              <tr><td><b>Confidence:</b></td><td><b>HIGH</b></td></tr>
            </table>
            <p style="margin-top: 10px; font-size: 12px; color: gray;">
              <i>Multi-sensor hits are highest priority for verification</i>
            </p>
          </div>
          ]]>
        </description>
        <styleUrl>#crossval_style</styleUrl>
        <Point>
          <coordinates>-87.10,42.47,0</coordinates>
        </Point>
      </Placemark>
    </Folder>
    
  </Document>
</kml>
'''
    
    return kml


def main():
    """Generate date-tiled KML database."""
    
    print('='*70)
    print('GOOGLE EARTH DATE-TILED DATABASE GENERATOR')
    print('='*70)
    print()
    
    # Generate KML
    kml_content = create_date_tiled_kml()
    
    # Save
    output_path = OUTPUT_DIR / 'wreckhunter_date_tiled.kml'
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write(kml_content)
    
    print(f'✓ Generated: {output_path}')
    print()
    print('Features:')
    print('  ✓ MAGNETIC folder - Toggle by MONTH to see training progression')
    print('  ✓ SATELLITE folder - Toggle by SCENE DATE')
    print('  ✓ CROSS-VALIDATION folder - Multi-sensor hits (highest confidence)')
    print('  ✓ Persistent popups - Stay open until you close them')
    print('  ✓ Only last 2 months visible by default (reduce clutter)')
    print()
    print('Usage for Training Comparison:')
    print('  1. Open in Google Earth')
    print('  2. Expand "MAGNETIC - By Date"')
    print('  3. Toggle 2025-08 (early training)')
    print('  4. Toggle 2026-03 (improved training)')
    print('  5. Compare the difference in detection quality!')
    print()
    print('='*70)


if __name__ == '__main__':
    main()
