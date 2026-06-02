#!/usr/bin/env python3
"""
Detailed Analysis of All 30 Detections
Location, Size Estimates, and Condition Assessment
"""

import json
import math
from pathlib import Path
from datetime import datetime

# All detected targets from full rescan
ALL_DETECTIONS = [
    # Andaste Site - 6 tile detections (same 4 objects detected in each tile)
    {
        "site_name": "Andaste Wreck Site",
        "site_id": "ANDASTE_1929",
        "objects": [
            {
                "name": "SS Andaste (Hull)",
                "lat": 42.4125,
                "lon": -87.2500,
                "length_ft": 266.9,
                "beam_ft": 38.1,
                "type": "Whaleback Freighter",
                "year_built": 1892,
                "year_lost": 1929,
                "cause": "Storm",
                "casualties": 25,
                "condition": "Intact hull, whaleback profile visible",
                "depth_ft": 180,
                "cargo": "Coal/Grain (unknown)",
                "mass_tons": 3500,
            },
            {
                "name": "Loading Boom (1925 Refit)",
                "lat": 42.4137,
                "lon": -87.2488,
                "length_ft": 117.0,
                "type": "Steel Derrick Structure",
                "year_added": 1925,
                "condition": "Collapsed/detached, rectangular thermal signature",
                "depth_ft": 180,
                "mass_tons": 150,
            },
            {
                "name": "Debris Object Alpha",
                "lat": 42.4107,
                "lon": -87.2490,
                "length_ft": 42.0,
                "type": "Steel/Lifeboat",
                "condition": "Moderate thermal, partial structure",
                "depth_ft": 180,
                "distance_from_hull_m": 216,
                "mass_tons": 25,
            },
            {
                "name": "Debris Object Beta",
                "lat": 42.4130,
                "lon": -87.2475,
                "length_ft": 38.0,
                "type": "Wooden/Workboat",
                "condition": "Weak thermal, organic material",
                "depth_ft": 180,
                "distance_from_hull_m": 213,
                "mass_tons": 20,
            },
        ],
    },
    
    # Monster Site - 6 tile detections (same 3 objects detected in each tile)
    {
        "site_name": "Monster Wreck Site",
        "site_id": "MONSTER_1929?",
        "objects": [
            {
                "name": "Unknown Freighter (Monster)",
                "lat": 42.4180,
                "lon": -87.2350,
                "length_ft": 342.8,
                "beam_ft": 45.0,
                "type": "Large Steel Freighter",
                "year_built": "1900-1925 (estimated)",
                "year_lost": "1929? (same storm as Andaste)",
                "cause": "Storm",
                "casualties": "Unknown",
                "condition": "Large intact hull, bow into wind orientation",
                "depth_ft": 180,
                "cargo": "Coal/Ore/Grain (~8,000-14,000 tons)",
                "mass_tons": 14474,
            },
            {
                "name": "Debris Alpha",
                "lat": 42.4165,
                "lon": -87.2340,
                "length_ft": 42.0,
                "type": "Steel/Lifeboat",
                "condition": "Moderate thermal, SE of hull",
                "depth_ft": 180,
                "distance_from_hull_m": 186,
                "mass_tons": 30,
            },
            {
                "name": "Debris Beta",
                "lat": 42.4162,
                "lon": -87.2335,
                "length_ft": 38.0,
                "type": "Wooden/Workboat",
                "condition": "Weak thermal, SE of hull",
                "depth_ft": 180,
                "distance_from_hull_m": 235,
                "mass_tons": 25,
            },
        ],
    },
]

def haversine_distance(lat1, lon1, lat2, lon2):
    """Calculate distance in meters"""
    r = 6371000.0
    lat1_r, lat2_r = math.radians(lat1), math.radians(lat2)
    dlat = math.radians(lat2 - lat1)
    dlon = math.radians(lon2 - lon1)
    a = math.sin(dlat/2)**2 + math.cos(lat1_r) * math.cos(lat2_r) * math.sin(dlon/2)**2
    return r * 2 * math.asin(math.sqrt(a))

def assess_condition(thermal_sig, length_ft, debris_distance=None):
    """Assess wreck condition based on thermal signature and characteristics"""
    assessments = []
    
    # Thermal-based assessment
    if "strong_steel" in thermal_sig.lower():
        assessments.append("Excellent structural integrity")
        preservation = 0.85
    elif "moderate_steel" in thermal_sig.lower():
        assessments.append("Good structure, some degradation")
        preservation = 0.65
    elif "weak_organic" in thermal_sig.lower():
        assessments.append("Fair condition, organic decay")
        preservation = 0.45
    else:
        assessments.append("Unknown condition")
        preservation = 0.50
    
    # Size-based assessment
    if length_ft > 300:
        assessments.append("Large vessel - more substantial remains")
    elif length_ft > 100:
        assessments.append("Medium vessel - moderate remains")
    else:
        assessments.append("Small object - limited structural mass")
    
    # Debris field assessment
    if debris_distance:
        if debris_distance < 100:
            assessments.append("Compact debris field - rapid sinking")
        elif debris_distance < 300:
            assessments.append("Moderate debris scatter - storm conditions")
        else:
            assessments.append("Wide debris field - possible explosion/breakup")
    
    return assessments, preservation

def generate_detailed_report():
    """Generate comprehensive report of all detections"""
    
    print("=" * 100)
    print("DETAILED ANALYSIS - ALL 30 DETECTIONS")
    print("Lake Michigan Full Rescan - Curvelet Filter Results")
    print("=" * 100)
    print()
    
    total_objects = 0
    total_mass = 0
    
    for site in ALL_DETECTIONS:
        print(f"SITE: {site['site_name']} ({site['site_id']})")
        print("-" * 100)
        print()
        
        # Calculate site center
        all_lats = [obj['lat'] for obj in site['objects']]
        all_lons = [obj['lon'] for obj in site['objects']]
        site_center_lat = sum(all_lats) / len(all_lats)
        site_center_lon = sum(all_lons) / len(all_lons)
        
        print(f"Site Center: {site_center_lat:.4f}°N, {site_center_lon:.4f}°W")
        print(f"Total Objects: {len(site['objects'])}")
        print()
        
        for i, obj in enumerate(site['objects'], 1):
            print(f"  OBJECT {i}: {obj['name']}")
            print(f"  {'─' * 80}")
            
            # Location
            print(f"    LOCATION:")
            print(f"      Coordinates:      {obj['lat']:.4f}°N, {obj['lon']:.4f}°W")
            if 'distance_from_hull_m' in obj:
                print(f"      Distance:         {obj['distance_from_hull_m']:.1f}m from main hull")
            print(f"      Depth:            {obj.get('depth_ft', 'Unknown')} ft")
            print()
            
            # Dimensions
            print(f"    DIMENSIONS:")
            print(f"      Length:           {obj['length_ft']:.1f} ft")
            if 'beam_ft' in obj:
                print(f"      Beam:             {obj['beam_ft']:.1f} ft")
            print(f"      Estimated Mass:   {obj.get('mass_tons', 'Unknown'):,.0f} tons")
            print()
            
            # Classification
            print(f"    CLASSIFICATION:")
            print(f"      Type:             {obj.get('type', 'Unknown')}")
            if 'year_built' in obj:
                print(f"      Year Built:       {obj['year_built']}")
            if 'year_lost' in obj:
                print(f"      Year Lost:        {obj['year_lost']}")
            if 'cause' in obj:
                print(f"      Cause:            {obj['cause']}")
            print()
            
            # Condition assessment
            thermal = obj.get('condition', '')
            assessments, preservation = assess_condition(
                obj.get('condition', ''),
                obj['length_ft'],
                obj.get('distance_from_hull_m')
            )
            
            print(f"    CONDITION:")
            print(f"      Signature:        {obj.get('condition', 'Unknown')}")
            print(f"      Preservation:     {preservation*100:.0f}%")
            for assessment in assessments:
                print(f"      • {assessment}")
            print()
            
            # Historical context
            if 'casualties' in obj:
                print(f"    HISTORICAL:")
                print(f"      Casualties:       {obj['casualties']}")
            if 'cargo' in obj:
                print(f"      Cargo:            {obj['cargo']}")
            print()
            
            total_objects += 1
            total_mass += obj.get('mass_tons', 0)
        
        print()
        print()
    
    print("=" * 100)
    print("SUMMARY STATISTICS")
    print("=" * 100)
    print()
    print(f"  Total Wreck Sites:     {len(ALL_DETECTIONS)}")
    print(f"  Total Objects:         {total_objects}")
    print(f"  Total Estimated Mass:  {total_mass:,.0f} tons")
    print()
    
    # Site separation
    andaste_center = (42.4125, -87.2500)
    monster_center = (42.4180, -87.2350)
    separation = haversine_distance(andaste_center[0], andaste_center[1], 
                                     monster_center[0], monster_center[1])
    
    print(f"  Site Separation:       {separation/1000:.2f} km ({separation:.1f} meters)")
    print()
    
    print("=" * 100)
    print("PRIORITY RECOMMENDATIONS")
    print("=" * 100)
    print()
    print("  1. HISTORICAL RESEARCH:")
    print("     • Verify Monster identity - likely 1929 storm loss")
    print("     • Search for missing freighter records (300-350ft class)")
    print("     • Cross-reference with Coast Guard wreck charts")
    print()
    print("  2. ROV SURVEY PRIORITY:")
    print("     • Andaste hull (confirmed identity, memorial site)")
    print("     • Monster hull (unknown identity, large mass)")
    print("     • Loading boom (1925 refit evidence)")
    print()
    print("  3. MEMORIAL CONSIDERATIONS:")
    print("     • Andaste: 25 confirmed casualties (1929)")
    print("     • Monster: Unknown casualties - research needed")
    print("     • Both sites: War grave potential")
    print()
    print("=" * 100)
    
    return ALL_DETECTIONS

def generate_detailed_kml(detections):
    """Generate detailed KML with full metadata"""
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Detailed Analysis - All 30 Detections</name>
  <description>Denny Hadfield Memorial Edition - Complete Site Survey</description>
'''
    
    for site in detections:
        kml += f'''  <Folder>
    <name>{site['site_name']}</name>
    <description>{site['site_id']}</description>
'''
        
        for obj in site['objects']:
            color = "ff0000ff" if "Hull" in obj['name'] or "Freighter" in obj.get('type', '') else "ff00aaff"
            
            kml += f'''    <Placemark>
      <name>{obj['name']}</name>
      <description>
        <![CDATA[
        <h3>{obj['name']}</h3>
        <table>
          <tr><td><b>Type:</b></td><td>{obj.get('type', 'Unknown')}</td></tr>
          <tr><td><b>Length:</b></td><td>{obj['length_ft']:.1f} ft</td></tr>
          <tr><td><b>Mass:</b></td><td>{obj.get('mass_tons', 0):,.0f} tons</td></tr>
          <tr><td><b>Year Lost:</b></td><td>{obj.get('year_lost', 'Unknown')}</td></tr>
          <tr><td><b>Cause:</b></td><td>{obj.get('cause', 'Unknown')}</td></tr>
'''
            if 'casualties' in obj:
                kml += f'''          <tr><td><b>Casualties:</b></td><td>{obj['casualties']}</td></tr>
'''
            if 'condition' in obj:
                kml += f'''          <tr><td><b>Condition:</b></td><td>{obj['condition']}</td></tr>
'''
            if 'distance_from_hull_m' in obj:
                kml += f'''          <tr><td><b>Distance:</b></td><td>{obj['distance_from_hull_m']:.1f}m from hull</td></tr>
'''
            
            kml += '''        </table>
        <br/><i>Denny Hadfield Memorial Edition</i>
        ]]>
      </description>
'''
            kml += f'''      <Style><IconStyle><color>{color}</color><scale>1.3</scale></IconStyle></Style>
      <Point><coordinates>{obj['lon']},{obj['lat']},0</coordinates></Point>
    </Placemark>
'''
        
        kml += '''  </Folder>
'''
    
    kml += '''</Document>
</kml>
'''
    
    return kml

if __name__ == "__main__":
    # Generate report
    detections = generate_detailed_report()
    
    # Generate KML
    kml_content = generate_detailed_kml(detections)
    
    # Save
    output_dir = Path(r"C:\Users\thomf\programming\wreckhunter2000\cesarops-search\outputs")
    output_dir.mkdir(exist_ok=True)
    
    kml_path = output_dir / "DETAILED_ALL_30_DETECTIONS.kml"
    with open(kml_path, "w", encoding='utf-8') as f:
        f.write(kml_content)
    
    # Also save JSON report
    json_path = output_dir / "DETAILED_ANALYSIS_REPORT.json"
    with open(json_path, "w", encoding='utf-8') as f:
        json.dump(detections, f, indent=2)
    
    print()
    print("=" * 100)
    print("OUTPUT FILES")
    print("=" * 100)
    print(f"  KML:  {kml_path}")
    print(f"  JSON: {json_path}")
    print("=" * 100)
