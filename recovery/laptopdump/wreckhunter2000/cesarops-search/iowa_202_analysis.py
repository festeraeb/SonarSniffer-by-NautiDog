#!/usr/bin/env python3
"""
Iowa-202 Inverse Projection Analysis
Target B (Monster) - High-Mass Package Freighter Identification
"""

import math
import json
from pathlib import Path

# Zion Constant from MASTER_FORENSIC_LEDGER V2.0
ZION_CONSTANT = 1.47

# Target B (Monster) detected parameters
TARGET_B = {
    "name": "Monster (Target B)",
    "lat": 42.4180,
    "lon": -87.2350,
    "detected_length_ft": 338.0,
    "drift_corrected_length_ft": 342.8,
    "estimated_mass_tons": 14474,
    "thermal_signature": "strong_steel",
    "condition": "Large intact hull, bow into wind orientation",
    "depth_ft": 180,
}

# SS Iowa historical profile (if this is a match)
SS_IOWA_PROFILE = {
    "name": "SS Iowa",
    "type": "Package Freighter / General Merchandise",
    "length_ft": 202.0,
    "beam_ft": 34.0,
    "gross_tonnage": 1847,
    "cargo_capacity_tons": 3500,
    "year_built": 1881,
    "year_lost": 1907,
    "cause": "Ice damage / Storm",
    "location_note": "Southern Lake Michigan",
    "casualties": 0,
    "special_features": [
        "Multiple cargo holds (general merchandise)",
        "Steel hull construction",
        "Ice-strengthened bow (Great Lakes service)",
        "Package freighter configuration"
    ]
}

def un_squeeze_analysis(detected_length, zion_constant):
    """
    Apply inverse projection to determine original vessel length
    Accounts for thermal bloom and current drift at depth
    """
    original_length = detected_length / zion_constant
    return original_length

def analyze_thermal_pattern(thermal_sig, mass_tons):
    """
    Analyze thermal signature for cargo type indicators
    Multiple point-source sinks = general merchandise
    Single uniform sink = bulk cargo (coal/ore/grain)
    """
    analysis = {
        "pattern": "unknown",
        "cargo_type": "unknown",
        "confidence": 0.0,
    }
    
    if "strong_steel" in thermal_sig.lower():
        # Check mass-to-length ratio for cargo type
        # Package freighters have lower density (mixed cargo)
        # Bulk carriers have higher density (uniform cargo)
        
        if mass_tons > 10000:
            # High mass could be:
            # 1. Large bulk carrier with full cargo
            # 2. Package freighter with dense merchandise
            analysis["pattern"] = "high_mass_steel"
            analysis["cargo_type"] = "Unknown (requires ROV verification)"
            analysis["confidence"] = 0.60
        else:
            analysis["pattern"] = "moderate_mass_steel"
            analysis["cargo_type"] = "General merchandise possible"
            analysis["confidence"] = 0.70
    
    return analysis

def check_ice_crush_profile(condition):
    """
    Search for structural fragmentation patterns consistent with ice damage
    """
    indicators = []
    
    if "bow" in condition.lower():
        indicators.append("Bow orientation noted")
    
    if "intact" in condition.lower():
        indicators.append("Hull appears intact - ice crush NOT evident")
        ice_damage_likelihood = "LOW"
    elif "fragment" in condition.lower() or "broken" in condition.lower():
        indicators.append("Structural fragmentation detected")
        ice_damage_likelihood = "HIGH"
    else:
        indicators.append("Condition inconclusive for ice damage")
        ice_damage_likelihood = "UNKNOWN"
    
    return indicators, ice_damage_likelihood

def run_iowa_202_projection():
    """
    Execute full Iowa-202 Inverse Projection analysis
    """
    print("=" * 100)
    print("IOWA-202 INVERSE PROJECTION ANALYSIS")
    print("Target B (Monster) - High-Mass Package Freighter Identification")
    print("=" * 100)
    print()
    
    # Step 1: The Un-Squeeze
    print("[STEP 1/3] THE 'UN-SQUEEZE' ANALYSIS")
    print("-" * 100)
    print()
    
    detected_length = TARGET_B["detected_length_ft"]
    corrected_length = TARGET_B["drift_corrected_length_ft"]
    
    # Apply Zion Constant to both detected and corrected lengths
    unsqueezed_detected = un_squeeze_analysis(detected_length, ZION_CONSTANT)
    unsqueezed_corrected = un_squeeze_analysis(corrected_length, ZION_CONSTANT)
    
    print(f"  Detected Length:        {detected_length:.1f} ft")
    print(f"  Drift-Corrected Length: {corrected_length:.1f} ft")
    print(f"  Zion Constant:          {ZION_CONSTANT:.2f}x")
    print()
    print(f"  Un-Squeezed (Detected):   {unsqueezed_detected:.1f} ft")
    print(f"  Un-Squeezed (Corrected):  {unsqueezed_corrected:.1f} ft")
    print()
    
    # Check against Iowa profile
    iowa_length = SS_IOWA_PROFILE["length_ft"]
    length_match_detected = abs(unsqueezed_detected - iowa_length) < 15
    length_match_corrected = abs(unsqueezed_corrected - iowa_length) < 15
    
    print(f"  SS Iowa Profile Length: {iowa_length:.1f} ft")
    print()
    print(f"  Length Match (Detected):  {'✓ YES' if length_match_detected else '✗ NO'} (diff: {abs(unsqueezed_detected - iowa_length):.1f} ft)")
    print(f"  Length Match (Corrected): {'✓ YES' if length_match_corrected else '✗ NO'} (diff: {abs(unsqueezed_corrected - iowa_length):.1f} ft)")
    print()
    
    # Step 2: Hardware Grid Analysis
    print("[STEP 2/3] THE 'HARDWARE' GRID ANALYSIS")
    print("-" * 100)
    print()
    
    thermal_analysis = analyze_thermal_pattern(TARGET_B["thermal_signature"], TARGET_B["estimated_mass_tons"])
    
    print(f"  Thermal Signature: {TARGET_B['thermal_signature']}")
    print(f"  Estimated Mass:    {TARGET_B['estimated_mass_tons']:,} tons")
    print()
    print(f"  Pattern:      {thermal_analysis['pattern']}")
    print(f"  Cargo Type:   {thermal_analysis['cargo_type']}")
    print(f"  Confidence:   {thermal_analysis['confidence']*100:.0f}%")
    print()
    
    # Mass check for Iowa
    # SS Iowa cargo capacity: ~3,500 tons
    # Detected mass: 14,474 tons (MUCH higher)
    iowa_cargo_capacity = 3500
    mass_ratio = TARGET_B["estimated_mass_tons"] / iowa_cargo_capacity
    
    print(f"  SS Iowa Cargo Capacity: ~{iowa_cargo_capacity:,} tons")
    print(f"  Detected Mass:          {TARGET_B['estimated_mass_tons']:,} tons")
    print(f"  Mass Ratio:             {mass_ratio:.1f}x Iowa's capacity")
    print()
    
    if mass_ratio > 3:
        print(f"  ⚠ MASS MISMATCH: Detected mass is {mass_ratio:.1f}x larger than Iowa's capacity")
        print(f"    This suggests either:")
        print(f"    • Much larger vessel than Iowa")
        print(f"    • Full bulk cargo (coal/ore) vs. package freight")
        print(f"    • Multiple vessels in same location")
    print()
    
    # Step 3: Ice-Crush Profile
    print("[STEP 3/3] THE 'ICE-CRUSH' PROFILE")
    print("-" * 100)
    print()
    
    ice_indicators, ice_likelihood = check_ice_crush_profile(TARGET_B["condition"])
    
    print(f"  Condition: {TARGET_B['condition']}")
    print()
    print("  Indicators:")
    for indicator in ice_indicators:
        print(f"    • {indicator}")
    print()
    print(f"  Ice Damage Likelihood: {ice_likelihood}")
    print()
    
    # Historical context for SS Iowa
    print("  SS Iowa Historical Context:")
    print(f"    • Built: {SS_IOWA_PROFILE['year_built']}")
    print(f"    • Lost: {SS_IOWA_PROFILE['year_lost']}")
    print(f"    • Cause: {SS_IOWA_PROFILE['cause']}")
    print(f"    • Location: {SS_IOWA_PROFILE['location_note']}")
    print()
    
    # Final Determination
    print("=" * 100)
    print("FINAL DETERMINATION")
    print("=" * 100)
    print()
    
    # Criteria from task:
    # - Un-Squeezed length ~202ft
    # - Mass >10k tons
    # → Label as SS Iowa
    
    mass_threshold = 10000
    mass_check = TARGET_B["estimated_mass_tons"] > mass_threshold
    
    print("  CRITERIA CHECK:")
    print(f"    ✓ Un-Squeezed Length ~202ft:  {'YES' if length_match_corrected else 'NO'} ({unsqueezed_corrected:.1f} ft)")
    print(f"    ✓ Mass >10,000 tons:          {'YES' if mass_check else 'NO'} ({TARGET_B['estimated_mass_tons']:,} tons)")
    print()
    
    # Scoring
    match_score = 0
    if length_match_corrected:
        match_score += 1
    if mass_check:
        match_score += 1
    if ice_likelihood == "HIGH":
        match_score += 1
    
    print(f"  MATCH SCORE: {match_score}/3")
    print()
    
    if length_match_corrected and mass_check:
        determination = "POSITIVE IDENTIFICATION"
        label = "SS IOWA (1881-1907)"
        confidence = 0.85
    elif mass_check and not length_match_corrected:
        determination = "PARTIAL MATCH - MASS ONLY"
        label = "UNKNOWN LARGE FREIGHTER (NOT Iowa)"
        confidence = 0.60
    elif length_match_corrected and not mass_check:
        determination = "PARTIAL MATCH - LENGTH ONLY"
        label = "POSSIBLE SS Iowa (mass discrepancy)"
        confidence = 0.50
    else:
        determination = "NO MATCH"
        label = "UNKNOWN VESSEL (NOT Iowa)"
        confidence = 0.40
    
    print(f"  DETERMINATION: {determination}")
    print(f"  LABEL: {label}")
    print(f"  CONFIDENCE: {confidence*100:.0f}%")
    print()
    
    # Discrepancy analysis
    if not length_match_corrected:
        print("  DISCREPANCY NOTES:")
        print(f"    • Un-Squeezed length ({unsqueezed_corrected:.1f} ft) does NOT match Iowa (202 ft)")
        print(f"    • Difference: {abs(unsqueezed_corrected - iowa_length):.1f} ft ({abs(unsqueezed_corrected - iowa_length)/iowa_length*100:.1f}%)")
        print(f"    • This suggests a vessel ~{unsqueezed_corrected:.0f}ft in original length")
        print(f"    • Iowa-class: 202ft | Detected-class: ~{unsqueezed_corrected:.0f}ft")
        print()
    
    if mass_check:
        print("  MASS ANALYSIS:")
        print(f"    • Detected mass ({TARGET_B['estimated_mass_tons']:,} tons) EXCEEDS 10k ton threshold")
        print(f"    • Consistent with large lake freighter (300-350ft class)")
        print(f"    • NOT consistent with Iowa-class package freighter (~3,500 tons cargo)")
        print()
    
    print("=" * 100)
    
    # Build result object
    result = {
        "analysis_type": "Iowa-202 Inverse Projection",
        "target": TARGET_B,
        "zion_constant": ZION_CONSTANT,
        "un_squeezed_detected": unsqueezed_detected,
        "un_squeezed_corrected": unsqueezed_corrected,
        "iowa_length": iowa_length,
        "length_match": length_match_corrected,
        "mass_check": mass_check,
        "mass_threshold": mass_threshold,
        "thermal_analysis": thermal_analysis,
        "ice_damage_likelihood": ice_likelihood,
        "determination": determination,
        "label": label,
        "confidence": confidence,
        "match_score": match_score,
    }
    
    return result

def generate_iowa_kml(result):
    """Generate KML with Iowa-202 analysis results"""
    
    kml = '''<?xml version="1.0" encoding="UTF-8"?>
<kml xmlns="http://www.opengis.net/kml/2.2">
<Document>
  <name>Iowa-202 Inverse Projection Analysis</name>
  <description>Target B Identification - Denny Hadfield Memorial Edition</description>
  
  <Folder>
    <name>Target B (Monster)</name>
'''
    
    # Determine color based on match
    if result["match_score"] >= 2:
        color = "ff00ff00"  # Green for positive match
    elif result["match_score"] == 1:
        color = "ff00aaff"  # Orange for partial
    else:
        color = "ff0000ff"  # Red for no match
    
    kml += f'''    <Placemark>
      <name>{result["label"]}</name>
      <description>
        <![CDATA[
        <h3>Iowa-202 Analysis Results</h3>
        <table>
          <tr><td><b>Determination:</b></td><td>{result["determination"]}</td></tr>
          <tr><td><b>Confidence:</b></td><td>{result["confidence"]*100:.0f}%</td></tr>
          <tr><td><b>Match Score:</b></td><td>{result["match_score"]}/3</td></tr>
          <tr><td><b>Detected Length:</b></td><td>{result["target"]["detected_length_ft"]:.1f} ft</td></tr>
          <tr><td><b>Un-Squeezed Length:</b></td><td>{result["un_squeezed_corrected"]:.1f} ft</td></tr>
          <tr><td><b>SS Iowa Length:</b></td><td>{result["iowa_length"]:.1f} ft</td></tr>
          <tr><td><b>Estimated Mass:</b></td><td>{result["target"]["estimated_mass_tons"]:,} tons</td></tr>
          <tr><td><b>Mass Threshold:</b></td><td>>10,000 tons</td></tr>
          <tr><td><b>Thermal Pattern:</b></td><td>{result["thermal_analysis"]["pattern"]}</td></tr>
          <tr><td><b>Ice Damage:</b></td><td>{result["ice_damage_likelihood"]}</td></tr>
        </table>
        <br/><i>Zion Constant: {result["zion_constant"]:.2f}x</i>
        <br/><i>Denny Hadfield Memorial Edition</i>
        ]]>
      </description>
      <Style><IconStyle><color>{color}</color><scale>1.5</scale></IconStyle></Style>
      <Point><coordinates>{result["target"]["lon"]},{result["target"]["lat"]},0</coordinates></Point>
    </Placemark>
  </Folder>
  
  <Folder>
    <name>SS Iowa Historical Profile (Reference)</name>
    <Placemark>
      <name>SS Iowa (1881-1907) - Reference Profile</name>
      <description>
        <![CDATA[
        <h3>SS Iowa Historical Data</h3>
        <table>
          <tr><td><b>Type:</b></td><td>Package Freighter</td></tr>
          <tr><td><b>Length:</b></td><td>{SS_IOWA_PROFILE["length_ft"]:.1f} ft</td></tr>
          <tr><td><b>Beam:</b></td><td>{SS_IOWA_PROFILE["beam_ft"]:.1f} ft</td></tr>
          <tr><td><b>Gross Tonnage:</b></td><td>{SS_IOWA_PROFILE["gross_tonnage"]:,} tons</td></tr>
          <tr><td><b>Cargo Capacity:</b></td><td>~{SS_IOWA_PROFILE["cargo_capacity_tons"]:,} tons</td></tr>
          <tr><td><b>Year Built:</b></td><td>{SS_IOWA_PROFILE["year_built"]}</td></tr>
          <tr><td><b>Year Lost:</b></td><td>{SS_IOWA_PROFILE["year_lost"]}</td></tr>
          <tr><td><b>Cause:</b></td><td>{SS_IOWA_PROFILE["cause"]}</td></tr>
          <tr><td><b>Location:</b></td><td>{SS_IOWA_PROFILE["location_note"]}</td></tr>
        </table>
        ]]>
      </description>
      <Style><IconStyle><color>ff888888</color><scale>1.0</scale></IconStyle></Style>
      <Point><coordinates>-87.2350,42.4180,0</coordinates></Point>
    </Placemark>
  </Folder>
</Document>
</kml>
'''
    
    return kml

if __name__ == "__main__":
    # Run analysis
    result = run_iowa_202_projection()
    
    # Generate KML
    kml_content = generate_iowa_kml(result)
    
    # Save
    output_dir = Path(r"C:\Users\thomf\programming\wreckhunter2000\cesarops-search\outputs")
    output_dir.mkdir(exist_ok=True)
    
    kml_path = output_dir / "IOWA_202_ANALYSIS.kml"
    with open(kml_path, "w", encoding='utf-8') as f:
        f.write(kml_content)
    
    # Save JSON report
    json_path = output_dir / "IOWA_202_REPORT.json"
    with open(json_path, "w", encoding='utf-8') as f:
        json.dump(result, f, indent=2, default=str)
    
    print()
    print("=" * 100)
    print("OUTPUT FILES")
    print("=" * 100)
    print(f"  KML:  {kml_path}")
    print(f"  JSON: {json_path}")
    print("=" * 100)
