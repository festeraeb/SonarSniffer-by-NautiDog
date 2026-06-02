#!/usr/bin/env python3
"""Cluster repeated thermal detections into single high-confidence pins."""

import json
import sys
from pathlib import Path
from math import sqrt

def distance_meters(lat1, lon1, lat2, lon2):
    """Haversine distance in meters."""
    from math import radians, sin, cos, asin
    
    lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
    dlat = lat2 - lat1
    dlon = lon2 - lon1
    a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
    return 6371000 * 2 * asin(sqrt(a))

def cluster_detections(input_json_path, output_json_path, distance_threshold=1000, zscore_tolerance=1.5):
    """
    Cluster detections within distance_threshold meters and similar Z-scores.
    
    Args:
        distance_threshold: Max distance in meters to consider same target (default 1000m = 1km VIIRS pixel)
        zscore_tolerance: Max Z-score difference to cluster together (default 1.5)
    """
    
    with open(input_json_path, 'r') as f:
        data = json.load(f)
    
    # Handle both grouped and ungrouped formats
    if 'warm_signatures' in data:
        all_detections = data.get('warm_signatures', []) + data.get('cold_signatures', [])
    else:
        all_detections = data.get('all_detections', [])
    
    clusters = []
    used = set()
    
    for i, det in enumerate(all_detections):
        if i in used:
            continue
        
        # Start new cluster
        cluster = {
            'observations': [det],
            'indices': [i]
        }
        
        # Find nearby similar detections
        for j, other in enumerate(all_detections):
            if j <= i or j in used:
                continue
            
            dist = distance_meters(det['lat'], det['lon'], other['lat'], other['lon'])
            zscore_diff = abs(det['zscore'] - other['zscore'])
            same_temp_class = (det['mode'] == other['mode'])
            
            if dist <= distance_threshold and zscore_diff <= zscore_tolerance and same_temp_class:
                cluster['observations'].append(other)
                cluster['indices'].append(j)
                used.add(j)
        
        used.add(i)
        clusters.append(cluster)
    
    # Build clustered detections
    clustered = []
    
    for cluster in clusters:
        obs = cluster['observations']
        n = len(obs)
        
        # Centroid position
        avg_lat = sum(o['lat'] for o in obs) / n
        avg_lon = sum(o['lon'] for o in obs) / n
        avg_zscore = sum(o['zscore'] for o in obs) / n
        
        # Confidence boost based on repeat observations
        if n == 1:
            confidence = 'LOW'
            color = 'yellow'
        elif n <= 3:
            confidence = 'MEDIUM'
            color = 'orange'
        elif n <= 5:
            confidence = 'HIGH'
            color = 'red'
        else:
            confidence = 'VERY_HIGH'
            color = 'darkred'
        
        # Temperature classification
        mode = obs[0]['mode']
        if mode == 'cold_sink':
            temp_class = 'COLD'
            kml_folder = 'Cold Thermal Signatures'
            color = 'blue' if n == 1 else 'darkblue' if n <= 3 else 'purple'
        else:
            temp_class = 'WARM'
            kml_folder = 'Warm Thermal Signatures'
        
        # Build observation details
        observation_dates = sorted(set(o['date'] for o in obs))
        zscore_range = [min(o['zscore'] for o in obs), max(o['zscore'] for o in obs)]
        
        clustered_detection = {
            'lat': round(avg_lat, 6),
            'lon': round(avg_lon, 6),
            'utm': obs[0]['utm'],
            'grid_ref': obs[0]['grid_ref'],
            'temperature_class': temp_class,
            'kml_folder': kml_folder,
            'kml_color': color,
            'observation_count': n,
            'confidence': confidence,
            'avg_zscore': round(avg_zscore, 3),
            'zscore_range': [round(zscore_range[0], 3), round(zscore_range[1], 3)],
            'observation_dates': observation_dates,
            'date_span_days': (max(observation_dates) if observation_dates else '') and (min(observation_dates) if observation_dates else ''),
            'sensors': sorted(set(o['sensor'] for o in obs)),
            'source_files': [o['source_file'] for o in obs]
        }
        
        clustered.append(clustered_detection)
    
    # Sort by confidence then Z-score
    confidence_order = {'VERY_HIGH': 0, 'HIGH': 1, 'MEDIUM': 2, 'LOW': 3}
    clustered.sort(key=lambda x: (confidence_order[x['confidence']], -abs(x['avg_zscore'])))
    
    output = {
        'run_at': data.get('run_at'),
        'bbox': data.get('bbox'),
        'files_processed': data.get('files_processed'),
        'original_detections': len(all_detections),
        'clustered_detections': len(clustered),
        'reduction_percent': round((1 - len(clustered)/len(all_detections)) * 100, 1),
        'clustering_params': {
            'distance_threshold_m': distance_threshold,
            'zscore_tolerance': zscore_tolerance
        },
        'detections': clustered,
        'metadata': {
            'confidence_levels': {
                'LOW': '1 observation (yellow/blue)',
                'MEDIUM': '2-3 observations (orange/darkblue)',
                'HIGH': '4-5 observations (red/purple)',
                'VERY_HIGH': '6+ observations (darkred/purple)'
            }
        }
    }
    
    with open(output_json_path, 'w') as f:
        json.dump(output, f, indent=2)
    
    print(f"[OK] Clustered {len(all_detections)} -> {len(clustered)} detections ({output['reduction_percent']}% reduction)")
    print(f"  - Distance threshold: {distance_threshold}m")
    print(f"  - Z-score tolerance: {zscore_tolerance}")
    
    # Stats by confidence
    for conf in ['VERY_HIGH', 'HIGH', 'MEDIUM', 'LOW']:
        count = sum(1 for d in clustered if d['confidence'] == conf)
        if count > 0:
            print(f"  - {conf}: {count}")

if __name__ == '__main__':
    input_path = Path('wreckhunter2000/outputs/straits_south_fox_historical/engine_results/straits_grouped_detections.json')
    output_path = Path('wreckhunter2000/outputs/straits_south_fox_historical/engine_results/straits_clustered_detections.json')
    
    cluster_detections(input_path, output_path, distance_threshold=1000, zscore_tolerance=1.5)
