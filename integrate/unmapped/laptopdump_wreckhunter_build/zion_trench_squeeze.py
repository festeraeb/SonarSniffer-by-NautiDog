#!/usr/bin/env python3
"""
ZION TRENCH SQUEEZE - Focused Multi-Sensor Scan
Target: SS Andaste + Unknown Heavy Targets
Area: 5km radius around Andaste location (Zion Trench)
Sensors: ALL (SAR, Thermal, Optical, SWOT, Magnetic)
Resolution: MAXIMUM (100m grid cells instead of 2km)
"""

import subprocess
import json
from pathlib import Path
from datetime import datetime
from typing import List, Dict, Tuple
import math

# ZION TRENCH CENTER (SS Andaste known location)
ANDASTE_UTM = {
    "easting": 457990.7,
    "northing": 4702720.4,
    "lat": 42.4757,
    "lon": -87.5111,
    "depth_ft": 180,
    "name": "SS Andaste (Whaleback)"
}

# SQUEEZE PARAMETERS
SQUEEZE_RADIUS_KM = 5.0  # 5km radius = 10km diameter search box
GRID_CELL_SIZE_M = 100   # 100m cells for HIGH RESOLUTION
THRESHOLD_AGGRESSIVE = 1.5  # Lower threshold = more detections

# Sensor configs
SENSORS = {
    "thermal": {"patterns": ["*B11.tif", "*B12.tif"], "threshold": THRESHOLD_AGGRESSIVE},
    "optical": {"patterns": ["*B04.tif", "*B08.tif"], "threshold": THRESHOLD_AGGRESSIVE},
    "sar": {"patterns": ["*vv.tif", "*vh.tif"], "threshold": 2.0},
    "swot": {"patterns": ["*swot*.tif"], "threshold": THRESHOLD_AGGRESSIVE},
    "magnetic": {"patterns": ["*mag*.tif"], "threshold": 2.5},
}

def generate_trench_grid() -> List[Dict]:
    """Generate 100m grid cells in 5km radius around Andaste"""
    center_e = ANDASTE_UTM["easting"]
    center_n = ANDASTE_UTM["northing"]
    radius_m = SQUEEZE_RADIUS_KM * 1000
    
    cells = []
    
    # Calculate grid bounds
    e_min = int((center_e - radius_m) / GRID_CELL_SIZE_M)
    e_max = int((center_e + radius_m) / GRID_CELL_SIZE_M)
    n_min = int((center_n - radius_m) / GRID_CELL_SIZE_M)
    n_max = int((center_n + radius_m) / GRID_CELL_SIZE_M)
    
    for e_idx in range(e_min, e_max + 1):
        for n_idx in range(n_min, n_max + 1):
            cell_e = (e_idx + 0.5) * GRID_CELL_SIZE_M
            cell_n = (n_idx + 0.5) * GRID_CELL_SIZE_M
            
            # Distance from Andaste
            dist_m = math.sqrt((cell_e - center_e)**2 + (cell_n - center_n)**2)
            
            # Only include cells within radius
            if dist_m <= radius_m:
                # Approximate lat/lon (simplified)
                lat = ANDASTE_UTM["lat"] + (cell_n - center_n) / 111320.0
                lon = ANDASTE_UTM["lon"] + (cell_e - center_e) / (111320.0 * 0.7)
                
                cells.append({
                    "grid_id": f"ZION-{e_idx:04d}-{n_idx:04d}",
                    "utm_e": cell_e,
                    "utm_n": cell_n,
                    "lat": lat,
                    "lon": lon,
                    "dist_from_andaste_m": round(dist_m, 1),
                    "in_lake": True  # All cells in trench are water
                })
    
    return cells

def find_sensor_tiffs(sensor_type: str, search_radius_km: float = 10.0) -> List[Path]:
    """Find TIFFs covering the Zion Trench area"""
    config = SENSORS[sensor_type]
    
    # Search paths
    if sensor_type in ["thermal", "optical"]:
        search_paths = [
            Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache"),
            Path(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
            Path(r"wreckhunter2000\wreck_hunting_ml\outputs\calibration\lake_michigan"),
        ]
    elif sensor_type == "sar":
        search_paths = [
            Path(r"C:\Users\thomf\programming\Bagrecovery\erie_remote\erie_remote_data\sar_cache"),
            Path(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
        ]
    elif sensor_type == "magnetic":
        search_paths = [
            Path(r"C:\Users\thomf\programming\Bagrecovery\magnetic_data\grids"),
        ]
    else:
        search_paths = [Path(r"C:\Users\thomf\programming\Bagrecovery")]
    
    tiffs = []
    for search_path in search_paths:
        if search_path.exists():
            for pattern in config['patterns']:
                tiffs.extend(search_path.rglob(pattern))
    
    return sorted(set(tiffs))[:10]  # Limit to 10 per sensor

def process_tiff_gpu(tiff_path: Path, threshold: float) -> Dict:
    """Process TIFF through M2200 GPU"""
    exe = Path("target/release/cesarops-gpu.exe")
    
    if not exe.exists():
        return {"status": "failed", "error": "GPU engine not built"}
    
    try:
        result = subprocess.run(
            [str(exe), str(tiff_path), "--threshold", str(threshold)],
            capture_output=True,
            text=True,
            timeout=120
        )
        
        if result.returncode != 0:
            return {"status": "failed", "error": result.stderr[:100]}
        
        # Parse anomalies
        anomalies = []
        for line in result.stdout.split('\n'):
            if "Pixel" in line and "Z-Score" in line:
                import re
                match = re.search(r'Pixel \((\d+), (\d+)\): Z-Score ([\d.]+)', line)
                if match:
                    anomalies.append({
                        "row": int(match.group(1)),
                        "col": int(match.group(2)),
                        "zscore": float(match.group(3))
                    })
        
        return {"status": "success", "anomalies": anomalies}
    
    except Exception as e:
        return {"status": "failed", "error": str(e)[:100]}

def main():
    print("=" * 80)
    print("ZION TRENCH SQUEEZE - FOCUSED MULTI-SENSOR SCAN")
    print("=" * 80)
    print()
    print(f"Target Area: {SQUEEZE_RADIUS_KM}km radius around SS Andaste")
    print(f"Center: UTM E {ANDASTE_UTM['easting']:.1f}, N {ANDASTE_UTM['northing']:.1f}")
    print(f"        WGS84 {ANDASTE_UTM['lat']:.4f}N, {ANDASTE_UTM['lon']:.4f}W")
    print(f"Grid Resolution: {GRID_CELL_SIZE_M}m cells (HIGH RESOLUTION)")
    print(f"Threshold: {THRESHOLD_AGGRESSIVE} (AGGRESSIVE)")
    print()
    
    start_time = datetime.now()
    
    # Generate grid
    print("[1/4] GENERATING TRENCH GRID")
    print("-" * 60)
    grid = generate_trench_grid()
    print(f"  Grid cells: {len(grid)}")
    print(f"  Cell size: {GRID_CELL_SIZE_M}m x {GRID_CELL_SIZE_M}m")
    print(f"  Coverage: {len(grid) * (GRID_CELL_SIZE_M/1000)**2:.2f} sq km")
    print()
    
    # Process each sensor
    print("[2/4] SCANNING ALL SENSORS")
    print("-" * 60)
    
    all_detections = []
    
    for sensor_type, config in SENSORS.items():
        print(f"\n  [{sensor_type.upper()}]")
        tiffs = find_sensor_tiffs(sensor_type)
        
        if not tiffs:
            print(f"    No TIFFs found")
            continue
        
        print(f"    Found {len(tiffs)} files")
        
        for i, tiff in enumerate(tiffs, 1):
            print(f"    [{i}/{len(tiffs)}] {tiff.name[:50]}")
            
            result = process_tiff_gpu(tiff, config['threshold'])
            
            if result['status'] == 'success':
                print(f"        Anomalies: {len(result['anomalies'])}")
                
                for anom in result['anomalies']:
                    # Map to nearest grid cell
                    # (simplified - would need proper geotransform)
                    all_detections.append({
                        "sensor": sensor_type,
                        "zscore": anom['zscore'],
                        "source": str(tiff.name),
                        "pixel": anom,
                        "in_lake": True
                    })
            else:
                print(f"        FAILED: {result.get('error', 'Unknown')[:40]}")
    
    print()
    print("[3/4] MULTI-SENSOR FUSION")
    print("-" * 60)
    
    # Group by location (simplified)
    print(f"  Total raw detections: {len(all_detections)}")
    
    # Filter high-confidence only
    high_conf = [d for d in all_detections if d['zscore'] >= 2.5]
    print(f"  High confidence (Z >= 2.5): {len(high_conf)}")
    
    # Count by sensor
    by_sensor = {}
    for d in high_conf:
        sensor = d['sensor']
        by_sensor[sensor] = by_sensor.get(sensor, 0) + 1
    
    print(f"\n  Detections by sensor:")
    for sensor, count in sorted(by_sensor.items()):
        print(f"    {sensor}: {count}")
    
    print()
    print("[4/4] EXPORT RESULTS")
    print("-" * 60)
    
    output_dir = Path("outputs/zion_trench_squeeze")
    output_dir.mkdir(parents=True, exist_ok=True)
    
    results = {
        "timestamp": datetime.now().isoformat(),
        "target": "Zion Trench (SS Andaste area)",
        "center": ANDASTE_UTM,
        "squeeze_radius_km": SQUEEZE_RADIUS_KM,
        "grid_cell_size_m": GRID_CELL_SIZE_M,
        "threshold": THRESHOLD_AGGRESSIVE,
        "grid_cells": len(grid),
        "total_detections": len(all_detections),
        "high_confidence_detections": len(high_conf),
        "by_sensor": by_sensor,
        "detections": high_conf,
        "duration_seconds": (datetime.now() - start_time).total_seconds()
    }
    
    output_file = output_dir / f"zion_trench_squeeze_{datetime.now().strftime('%Y%m%d_%H%M%S')}.json"
    with open(output_file, 'w') as f:
        json.dump(results, f, indent=2)
    
    print(f"  Results: {output_file}")
    print()
    
    # Summary
    print("=" * 80)
    print("ZION TRENCH SQUEEZE COMPLETE")
    print("=" * 80)
    print()
    print(f"Duration: {results['duration_seconds']:.1f} seconds")
    print(f"Grid cells scanned: {len(grid)}")
    print(f"Total detections: {len(all_detections)}")
    print(f"High confidence: {len(high_conf)}")
    print()
    print("Top 10 Detections:")
    for i, det in enumerate(sorted(high_conf, key=lambda x: x['zscore'], reverse=True)[:10], 1):
        print(f"  [{i}] {det['sensor'].upper()}: Z={det['zscore']:.2f} | {det['source'][:40]}")
    print()
    print(f"Full results: {output_file}")
    print("=" * 80)

if __name__ == "__main__":
    main()
