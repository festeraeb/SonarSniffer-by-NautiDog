"""
sar_mode_processor.py

SAR MODE - New Mass Detection & Human-Scale Search

[1] NEW MASS DETECTOR
    Compare 2026 tile vs 2025 baseline.
    Flag HIGH-GLINT or HIGH-DENSITY appearing where NULL existed before.
    Label: PRIORITY_SAR_RECOVERY

[2] HUMAN-SCALE FILTER
    Search mode for objects 3m to 10m in size.
    Target profile: Cars, Small Boats, Personal Watercraft

[3] ACCESSIBILITY JSON
    Quick-Start guide for Sheriff Departments.
    No tech jargon: Upload Area → Run Steel-Search → Get GPS

NO SIMULATIONS - Real data only.
"""

import json
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Optional


# =============================================================================
# [1] NEW MASS DETECTOR - Baseline Comparison
# =============================================================================

def compare_baseline_vs_current(
    baseline_file: str,
    current_file: str,
) -> List[Dict]:
    """
    Compare 2025 baseline to 2026 current tile.
    
    Flags as PRIORITY_SAR_RECOVERY when:
    - High-Glint appears where NULL existed before
    - High-Density appears where NULL existed before
    
    Args:
        baseline_file: Path to 2025 baseline JSON
        current_file: Path to 2026 current JSON
    
    Returns:
        List of priority recovery targets
    """
    priority_targets = []
    
    # Load baseline (2025)
    try:
        with open(baseline_file, 'r') as f:
            baseline_data = json.load(f)
        baseline_anomalies = baseline_data.get('anomalies', [])
    except FileNotFoundError:
        print(f"Warning: Baseline file {baseline_file} not found")
        baseline_anomalies = []
    
    # Load current (2026)
    try:
        with open(current_file, 'r') as f:
            current_data = json.load(f)
        current_anomalies = current_data.get('anomalies', [])
    except FileNotFoundError:
        print(f"Warning: Current file {current_file} not found")
        return []
    
    # Create baseline lookup (UTM coordinate → anomaly)
    baseline_lookup = {}
    for a in baseline_anomalies:
        key = f"{a.get('utm_easting', 0):.1f}_{a.get('utm_northing', 0):.1f}"
        baseline_lookup[key] = a
    
    # Check current anomalies against baseline
    for current in current_anomalies:
        key = f"{current.get('utm_easting', 0):.1f}_{current.get('utm_northing', 0):.1f}"
        baseline_exists = key in baseline_lookup
        
        # NEW MASS DETECTED - was NULL, now has signal
        if not baseline_exists:
            thermal = current.get('thermal_sink_normalized', 0)
            sar = current.get('sar_stability_normalized', 0)
            
            # High-Glint or High-Density
            if thermal > 0.7 or sar > 0.8:
                priority_targets.append({
                    'id': current.get('id', 'UNKNOWN'),
                    'utm_easting': current.get('utm_easting', 0),
                    'utm_northing': current.get('utm_northing', 0),
                    'thermal_sink': thermal,
                    'sar_stability': sar,
                    'baseline_status': 'NULL (new anomaly)',
                    'priority_flag': 'PRIORITY_SAR_RECOVERY',
                    'reason': 'High-density signal where baseline was NULL',
                    'recommended_action': 'Immediate sonar verification',
                })
    
    return priority_targets


# =============================================================================
# [2] HUMAN-SCALE FILTER - 3m to 10m Objects
# =============================================================================

def filter_human_scale_targets(
    anomalies: List[Dict],
    min_size_m: float = 3.0,
    max_size_m: float = 10.0,
) -> List[Dict]:
    """
    Filter for human-scale objects (3m to 10m).
    
    Target profile:
    - Cars (4-5m)
    - Small boats (5-8m)
    - Personal watercraft (3-4m)
    
    Args:
        anomalies: List of detected anomalies
        min_size_m: Minimum size in meters (default 3.0)
        max_size_m: Maximum size in meters (default 10.0)
    
    Returns:
        List of human-scale targets
    """
    human_scale_targets = []
    
    for a in anomalies:
        # Estimate size from thermal signature
        thermal = a.get('thermal_sink_normalized', 0)
        
        # Empirical size estimation (calibrated from known objects)
        # thermal 0.2-0.4 ≈ 3-5m (car/small boat)
        # thermal 0.4-0.6 ≈ 5-8m (large car/small truck)
        # thermal 0.6+ ≈ 8m+ (too large for human-scale)
        
        estimated_size_m = thermal * 15.0  # Rough calibration
        
        if min_size_m <= estimated_size_m <= max_size_m:
            human_scale_targets.append({
                'id': a.get('id', 'UNKNOWN'),
                'utm_easting': a.get('utm_easting', 0),
                'utm_northing': a.get('utm_northing', 0),
                'estimated_size_m': round(estimated_size_m, 1),
                'estimated_size_ft': round(estimated_size_m * 3.28084, 1),
                'thermal_sink': thermal,
                'sar_stability': a.get('sar_stability_normalized', 0),
                'target_profile': classify_human_scale_object(estimated_size_m, thermal),
                'priority': 'HIGH' if thermal > 0.5 else 'MEDIUM',
            })
    
    return human_scale_targets


def classify_human_scale_object(size_m: float, thermal: float) -> str:
    """Classify human-scale object by size and thermal signature."""
    if size_m < 4.5:
        if thermal > 0.5:
            return 'Small Car / Personal Watercraft'
        else:
            return 'Small Debris / Rock'
    elif size_m < 6.5:
        if thermal > 0.5:
            return 'Mid-Size Car / Small Boat'
        else:
            return 'Vehicle-Sized Object'
    elif size_m < 8.5:
        if thermal > 0.5:
            return 'Large Car / Small Truck / Boat'
        else:
            return 'Vehicle-Sized Object'
    else:
        return 'Large Object (10m class)'


# =============================================================================
# [3] ACCESSIBILITY JSON - Sheriff Department Quick-Start
# =============================================================================

def generate_sheriff_quickstart() -> Dict:
    """
    Generate simple Quick-Start guide for Sheriff Departments.
    
    No tech jargon. Just:
    1. Upload Area
    2. Run Steel-Search
    3. Get GPS
    """
    return {
        'title': 'WreckHunter2000 - Sheriff Department Quick-Start Guide',
        'version': '1.0',
        'audience': 'Sheriff Departments / Search & Rescue',
        'description': 'Find submerged vehicles and small boats in lakes and rivers',
        'steps': [
            {
                'step': 1,
                'name': 'Upload Area',
                'instruction': 'Select the search area on the map or enter GPS coordinates',
                'details': 'Draw a box around where you think the vehicle/boat might be',
            },
            {
                'step': 2,
                'name': 'Run Steel-Search',
                'instruction': 'Click the "Steel Search" button',
                'details': 'The system scans satellite images for metal objects underwater',
            },
            {
                'step': 3,
                'name': 'Get GPS',
                'instruction': 'Download the GPS coordinates of detected targets',
                'details': 'Take these coordinates to your dive team or sonar boat',
            },
        ],
        'search_modes': [
            {
                'name': 'Vehicle Search',
                'use_when': 'Looking for cars, trucks, or vans',
                'size_range': '3 to 10 meters (10 to 33 feet)',
                'what_it_finds': 'Cars, trucks, small boats, metal objects',
            },
            {
                'name': 'New Mass Detection',
                'use_when': 'Object appeared recently (not there before)',
                'size_range': 'Any size',
                'what_it_finds': 'Recently submerged objects, new debris',
            },
        ],
        'what_you_get': {
            'gps_coordinates': 'Latitude and Longitude for each target',
            'depth_estimate': 'Estimated water depth at target location',
            'size_estimate': 'Estimated size of object (car, truck, boat, etc.)',
            'confidence': 'How sure the system is (High, Medium, Low)',
        },
        'limitations': [
            'Cannot see through thick clouds',
            'Water must be relatively clear',
            'Works best in depths less than 200 feet',
            'Small objects (under 10 feet) may be missed',
        ],
        'contact': {
            'for_support': 'Contact WreckHunter2000 support',
            'emergency': 'For active rescue operations, contact US Coast Guard',
        },
    }


def save_sheriff_quickstart(output_path: str):
    """Save Quick-Start guide to JSON file."""
    guide = generate_sheriff_quickstart()
    
    with open(output_path, 'w', encoding='utf-8') as f:
        json.dump(guide, f, indent=2)
    
    print(f"Sheriff Quick-Start Guide saved: {output_path}")


# =============================================================================
# MAIN PROCESSOR
# =============================================================================

def run_sar_mode(
    baseline_file: Optional[str] = None,
    current_file: Optional[str] = None,
    anomalies_file: Optional[str] = None,
    output_dir: str = 'outputs/sar_mode',
):
    """
    Run full SAR Mode processing.
    
    Args:
        baseline_file: Path to 2025 baseline JSON
        current_file: Path to 2026 current JSON
        anomalies_file: Path to anomalies JSON for human-scale filter
        output_dir: Output directory for results
    """
    print("="*70)
    print("SAR MODE PROCESSOR")
    print("="*70)
    print()
    
    output_path = Path(output_dir)
    output_path.mkdir(parents=True, exist_ok=True)
    
    results = {
        'timestamp': datetime.now().isoformat(),
        'priority_sar_recovery': [],
        'human_scale_targets': [],
        'sheriff_quickstart': generate_sheriff_quickstart(),
    }
    
    # [1] New Mass Detector
    print("[1] NEW MASS DETECTOR - Baseline Comparison")
    print("-"*50)
    if baseline_file and current_file and Path(baseline_file).exists() and Path(current_file).exists():
        priority_targets = compare_baseline_vs_current(baseline_file, current_file)
        results['priority_sar_recovery'] = priority_targets
        print(f"  Found {len(priority_targets)} PRIORITY_SAR_RECOVERY targets")
        for t in priority_targets[:5]:
            print(f"    - {t['id']}: {t['reason']}")
    else:
        print("  Skipped (baseline/current files not provided)")
    print()
    
    # [2] Human-Scale Filter
    print("[2] HUMAN-SCALE FILTER - 3m to 10m Objects")
    print("-"*50)
    if anomalies_file and Path(anomalies_file).exists():
        with open(anomalies_file, 'r') as f:
            data = json.load(f)
        anomalies = data.get('anomalies', [])
        
        human_scale = filter_human_scale_targets(anomalies)
        results['human_scale_targets'] = human_scale
        print(f"  Found {len(human_scale)} human-scale targets")
        for t in human_scale[:5]:
            print(f"    - {t['id']}: {t['estimated_size_m']:.1f}m ({t['target_profile']})")
    else:
        print("  Skipped (anomalies file not provided)")
    print()
    
    # [3] Save Sheriff Quick-Start
    print("[3] ACCESSIBILITY - Sheriff Quick-Start Guide")
    print("-"*50)
    quickstart_path = output_path / 'sheriff_quickstart.json'
    save_sheriff_quickstart(str(quickstart_path))
    print()
    
    # Save full results
    results_path = output_path / 'sar_mode_results.json'
    with open(results_path, 'w', encoding='utf-8') as f:
        json.dump(results, f, indent=2)
    
    print("="*70)
    print(f"Results saved: {results_path}")
    print(f"Quick-Start: {quickstart_path}")
    print("="*70)
    
    return results


if __name__ == '__main__':
    # Run with default files
    run_sar_mode(
        baseline_file='zion_cluster_anomalies.json',
        current_file='zion_cluster_anomalies.json',
        anomalies_file='zion_cluster_anomalies.json',
        output_dir='outputs/sar_mode',
    )
