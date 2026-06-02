#!/usr/bin/env python3
"""Group all thermal detections by temperature classification for Google Earth KML toggles."""

import json
import sys
from pathlib import Path

def group_detections(input_json_path, output_json_path):
    """Group detections into warm (positive Z) and cold (negative Z) categories."""
    
    with open(input_json_path, 'r') as f:
        data = json.load(f)
    
    all_detections = data.get('all_detections', [])
    
    warm_signatures = []
    cold_signatures = []
    
    for detection in all_detections:
        zscore = detection.get('zscore', 0)
        mode = detection.get('mode', '')
        
        detection_copy = detection.copy()
        
        if mode == 'cold_sink' or zscore < 0:
            detection_copy['temperature_class'] = 'COLD'
            detection_copy['kml_folder'] = 'Cold Thermal Signatures'
            cold_signatures.append(detection_copy)
        else:
            detection_copy['temperature_class'] = 'WARM'
            detection_copy['kml_folder'] = 'Warm Thermal Signatures'
            warm_signatures.append(detection_copy)
    
    output = {
        'run_at': data.get('run_at'),
        'bbox': data.get('bbox'),
        'files_processed': data.get('files_processed'),
        'total_detections': len(all_detections),
        'warm_count': len(warm_signatures),
        'cold_count': len(cold_signatures),
        'warm_signatures': warm_signatures,
        'cold_signatures': cold_signatures,
        'metadata': {
            'description': 'Thermal detections grouped by temperature signature',
            'warm_definition': 'Positive Z-scores or thermal_contrast mode (warmer than surroundings)',
            'cold_definition': 'Negative Z-scores or cold_sink mode (colder than surroundings)',
            'kml_usage': 'Use kml_folder field to create separate toggleable layers in Google Earth'
        }
    }
    
    with open(output_json_path, 'w') as f:
        json.dump(output, f, indent=2)
    
    print(f"[OK] Grouped {len(all_detections)} detections:")
    print(f"  - Warm signatures: {len(warm_signatures)}")
    print(f"  - Cold signatures: {len(cold_signatures)}")
    print(f"[OK] Saved to: {output_json_path}")

if __name__ == '__main__':
    if len(sys.argv) < 2:
        input_path = Path('wreckhunter2000/outputs/straits_south_fox_historical/engine_results/straits_engine_master_report.json')
        output_path = Path('wreckhunter2000/outputs/straits_south_fox_historical/engine_results/straits_grouped_detections.json')
    else:
        input_path = Path(sys.argv[1])
        output_path = Path(sys.argv[2]) if len(sys.argv) > 2 else input_path.parent / 'grouped_detections.json'
    
    group_detections(input_path, output_path)
