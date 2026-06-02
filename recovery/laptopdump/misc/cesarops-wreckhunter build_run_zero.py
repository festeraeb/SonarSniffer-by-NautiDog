#!/usr/bin/env python3
"""
RUN ZERO - BASELINE SCAN
Log everything to database, check repeatability
"""

import sqlite3
import json
import subprocess
import sys
import re
import platform
from pathlib import Path
from datetime import datetime

# ============================================================================
# SYSTEM INFO
# ============================================================================

def get_system_info():
    """Get system hardware information"""
    info = {
        'gpu_name': 'Unknown',
        'gpu_vendor': 'Unknown',
        'gpu_type': 'Unknown',
        'cpu_cores': 0,
        'system_ram_gb': 0.0
    }
    
    # CPU cores
    info['cpu_cores'] = platform.processor().count('Core') if 'Core' in platform.processor() else 0
    if info['cpu_cores'] == 0:
        import os
        info['cpu_cores'] = os.cpu_count() or 0
    
    # RAM
    try:
        import psutil
        info['system_ram_gb'] = psutil.virtual_memory().total / (1024**3)
    except:
        # Fallback - try to parse from system info
        try:
            result = subprocess.run(['wmic', 'OS', 'get', 'TotalVisibleMemorySize'], 
                                  capture_output=True, text=True)
            if result.stdout:
                lines = result.stdout.strip().split('\n')
                if len(lines) > 1:
                    ram_kb = int(lines[1].strip())
                    info['system_ram_gb'] = ram_kb / (1024**2)
        except:
            info['system_ram_gb'] = 48.0  # Default from user input
    
    # GPU info from scanner output (we'll parse it)
    # For now, set defaults based on user's system
    info['gpu_name'] = 'Intel HD Graphics 630'
    info['gpu_vendor'] = 'Intel'
    info['gpu_type'] = 'Integrated'
    
    return info

# ============================================================================
# CONFIGURATION
# ============================================================================

DATA_DIR = Path(r".\wreckhunter2000\data\cache\census_raw\2021_low_water")
OUTPUT_BASE = Path(r".\outputs\run_zero")
DB_PATH = OUTPUT_BASE / "cesarops_runs.db"

OUTPUT_BASE.mkdir(parents=True, exist_ok=True)

# ============================================================================
# DATABASE
# ============================================================================

def init_db():
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    # Simple schema - just what we need
    cursor.execute('''
        CREATE TABLE IF NOT EXISTS runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_name TEXT,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            detection_count INTEGER,
            chunking_enabled BOOLEAN,
            overlap_percent REAL,
            duration_seconds REAL,
            
            -- Hardware info
            gpu_name TEXT,
            gpu_vendor TEXT,
            gpu_type TEXT,
            cpu_cores INTEGER,
            system_ram_gb REAL
        )
    ''')
    
    cursor.execute('''
        CREATE TABLE IF NOT EXISTS detections (
            detection_id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_id INTEGER,
            utm_easting REAL,
            utm_northing REAL,
            wgs84_lat REAL,
            wgs84_lon REAL,
            score REAL,
            classification TEXT,
            aluminum_ratio REAL,
            thermal_delta REAL,
            pixel_row INTEGER,
            pixel_col INTEGER,
            grid_ref TEXT,
            FOREIGN KEY (run_id) REFERENCES runs(run_id)
        )
    ''')
    
    conn.commit()
    conn.close()
    print("[OK] Database initialized")

def log_run(run_name, detections, chunking=True, overlap=0.1, duration=0.0, 
            gpu_name="", gpu_vendor="", gpu_type="", cpu_cores=0, system_ram_gb=0.0):
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    cursor.execute('''
        INSERT INTO runs (
            run_name, detection_count, chunking_enabled, overlap_percent, duration_seconds,
            gpu_name, gpu_vendor, gpu_type, cpu_cores, system_ram_gb
        ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
    ''', (run_name, len(detections), chunking, overlap, duration, 
          gpu_name, gpu_vendor, gpu_type, cpu_cores, system_ram_gb))
    
    run_id = cursor.lastrowid
    
    for det in detections:
        cursor.execute('''
            INSERT INTO detections (
                run_id, utm_easting, utm_northing, wgs84_lat, wgs84_lon,
                score, classification, aluminum_ratio, thermal_delta,
                pixel_row, pixel_col, grid_ref
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ''', (
            run_id,
            det.get('utm_easting', 0),
            det.get('utm_northing', 0),
            det.get('wgs84_lat', 0),
            det.get('wgs84_lon', 0),
            det.get('score', 0),
            det.get('classification', ''),
            det.get('aluminum_ratio', 0),
            det.get('thermal_delta', 0),
            det.get('pixel_row', 0),
            det.get('pixel_col', 0),
            det.get('grid_ref', ''),
        ))
    
    conn.commit()
    conn.close()
    return run_id

# ============================================================================
# PARSE KML
# ============================================================================

def parse_kml(kmz_path):
    import zipfile
    
    detections = []
    
    if not kmz_path.exists():
        return detections
    
    # KMZ is a zip file - extract KML
    try:
        with zipfile.ZipFile(str(kmz_path), 'r') as zip_ref:
            # Find the KML file inside
            kml_name = None
            for name in zip_ref.namelist():
                if name.endswith('.kml'):
                    kml_name = name
                    break
            
            if not kml_name:
                print(f"  No KML found in {kmz_path}")
                return detections
            
            content = zip_ref.read(kml_name).decode('utf-8')
    except Exception as e:
        print(f"  Error reading KMZ: {e}")
        return detections
    
    placemarks = re.findall(r'<Placemark>(.*?)</Placemark>', content, re.DOTALL)
    
    for pm in placemarks:
        det = {}
        
        # Coordinates
        coords_match = re.search(r'<coordinates>([^<]+)</coordinates>', pm)
        if coords_match:
            coords = coords_match.group(1).strip().split(',')
            if len(coords) >= 2:
                det['wgs84_lon'] = float(coords[0])
                det['wgs84_lat'] = float(coords[1])
        
        # UTM
        utm_e_match = re.search(r'E: ([\d.]+)m', pm)
        utm_n_match = re.search(r'N: ([\d.]+)m', pm)
        if utm_e_match:
            det['utm_easting'] = float(utm_e_match.group(1))
        if utm_n_match:
            det['utm_northing'] = float(utm_n_match.group(1))
        
        # Score
        score_match = re.search(r'<b>Score:</b></td><td>([\d.]+)', pm)
        if score_match:
            det['score'] = float(score_match.group(1))
        
        # Classification
        class_match = re.search(r'<b>Classification:</b></td><td>([^<]+)', pm)
        if class_match:
            det['classification'] = class_match.group(1).strip()
        
        # Aluminum
        alum_match = re.search(r'<b>B08/B04 Ratio:</b></td><td>([\d.]+)', pm)
        if alum_match:
            det['aluminum_ratio'] = float(alum_match.group(1))
        
        # Thermal
        therm_match = re.search(r'<b>Thermal Delta:</b></td><td>([\d.]+)', pm)
        if therm_match:
            det['thermal_delta'] = float(therm_match.group(1))
        
        # Pixel position
        row_match = re.search(r'Row: (\d+), Col: (\d+)', pm)
        if row_match:
            det['pixel_row'] = int(row_match.group(1))
            det['pixel_col'] = int(row_match.group(2))
        
        # Grid ref
        grid_match = re.search(r'<b>Grid Ref:</b></td><td>([^<]+)', pm)
        if grid_match:
            det['grid_ref'] = grid_match.group(1).strip()
        
        detections.append(det)
    
    return detections

# ============================================================================
# RUN SCANNER
# ============================================================================

def run_scanner(run_name, output_dir, overlap=0.1):
    print(f"\n[{run_name}]")
    print("=" * 80)
    
    output_path = OUTPUT_BASE / output_dir
    output_path.mkdir(parents=True, exist_ok=True)
    
    # Verify input data exists
    tif_files = list(DATA_DIR.glob("*.tif"))
    print(f"Input directory: {DATA_DIR}")
    print(f"TIFF files found: {len(tif_files)}")
    if tif_files:
        print(f"  First file: {tif_files[0].name} ({tif_files[0].stat().st_size / 1e6:.1f} MB)")
    
    cmd = [
        r".\target\release\cesarops-search.exe",
        "scan", "michigan",
        "--input", str(DATA_DIR),
        "--output", str(output_path),
        "--min-confidence", "0.0",
        "--overlap", str(overlap)
    ]
    
    print(f"\nCommand: {' '.join(cmd)}")
    
    start_time = datetime.now()
    result = subprocess.run(cmd, capture_output=True, text=True, timeout=600)
    end_time = datetime.now()
    duration = (end_time - start_time).total_seconds()
    
    print(f"Duration: {duration:.2f}s")
    
    if result.returncode != 0:
        print(f"ERROR: {result.stderr}")
        return None, 0
    
    return output_path, duration

# ============================================================================
# ANALYZE
# ============================================================================

def analyze():
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    print("\n" + "=" * 80)
    print("ANALYSIS RESULTS")
    print("=" * 80)
    
    # Detection counts with hardware info
    cursor.execute('''
        SELECT run_name, detection_count, duration_seconds, gpu_name, gpu_type, timestamp 
        FROM runs 
        ORDER BY run_id
    ''')
    
    print("\nDetection Counts:")
    for row in cursor.fetchall():
        print(f"  {row[0]}: {row[1]} detections in {row[2]:.2f}s [{row[3]} {row[4]}] ({row[5]})")
    
    # Compare first 10 detections between runs
    cursor.execute('''
        SELECT d1.utm_easting, d1.utm_northing, d2.utm_easting, d2.utm_northing,
               d1.score, d2.score
        FROM detections d1
        JOIN detections d2 ON d1.detection_id = d2.detection_id
        WHERE d1.run_id = 1 AND d2.run_id = 2
        LIMIT 10
    ''')
    
    print("\nPosition Comparison (First 10 detections):")
    print("  Run 1 (E, N, Score)              | Run 2 (E, N, Score)")
    print("  " + "-" * 75)
    for row in cursor.fetchall():
        print(f"  {row[0]:10.2f}, {row[1]:10.2f}, {row[4]:.3f}  |  {row[2]:10.2f}, {row[3]:10.2f}, {row[5]:.3f}")
    
    # Check identical detections
    cursor.execute('''
        SELECT COUNT(*)
        FROM detections d1
        JOIN detections d2 ON 
            ABS(d1.utm_easting - d2.utm_easting) < 1.0 AND
            ABS(d1.utm_northing - d2.utm_northing) < 1.0 AND
            ABS(d1.score - d2.score) < 0.01
        WHERE d1.run_id = 1 AND d2.run_id = 2
    ''')
    
    identical = cursor.fetchone()[0]
    print(f"\nIdentical detections (within 1m + 0.01 score): {identical}")
    
    conn.close()

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("=" * 80)
    print("RUN ZERO - BASELINE SCAN")
    print("=" * 80)
    print("\nLogging ALL detections (min-confidence = 0.0)")
    print("Database:", DB_PATH)
    
    # Get system info once
    sys_info = get_system_info()
    print(f"\nSystem: {sys_info['gpu_name']} ({sys_info['gpu_vendor']} {sys_info['gpu_type']})")
    print(f"CPU: {sys_info['cpu_cores']} cores, RAM: {sys_info['system_ram_gb']:.1f} GB")
    
    init_db()
    
    # Run 1-2: Chunked (512x512, 10% overlap) - original config
    output1, duration1 = run_scanner("Run 1 - Chunked", "run1_chunked", overlap=0.1)
    if output1:
        kmz1 = output1 / "LAKE_MICHIGAN_SOUTH_CENSUS.kmz"
        dets1 = parse_kml(kmz1)
        log_run("Run 1 - Chunked", dets1, chunking=True, overlap=0.1, duration=duration1,
                gpu_name=sys_info['gpu_name'], gpu_vendor=sys_info['gpu_vendor'],
                gpu_type=sys_info['gpu_type'], cpu_cores=sys_info['cpu_cores'],
                system_ram_gb=sys_info['system_ram_gb'])
        print(f"Logged {len(dets1)} detections")
    
    output2, duration2 = run_scanner("Run 2 - Chunked", "run2_chunked", overlap=0.1)
    if output2:
        kmz2 = output2 / "LAKE_MICHIGAN_SOUTH_CENSUS.kmz"
        dets2 = parse_kml(kmz2)
        log_run("Run 2 - Chunked", dets2, chunking=True, overlap=0.1, duration=duration2,
                gpu_name=sys_info['gpu_name'], gpu_vendor=sys_info['gpu_vendor'],
                gpu_type=sys_info['gpu_type'], cpu_cores=sys_info['cpu_cores'],
                system_ram_gb=sys_info['system_ram_gb'])
        print(f"Logged {len(dets2)} detections")
    
    # Run 3-4: Full tile (no chunking, no overlap)
    output3, duration3 = run_scanner("Run 3 - Full Tile", "run3_fulltile", overlap=0.0)
    if output3:
        kmz3 = output3 / "LAKE_MICHIGAN_SOUTH_CENSUS.kmz"
        dets3 = parse_kml(kmz3)
        log_run("Run 3 - Full Tile", dets3, chunking=False, overlap=0.0, duration=duration3,
                gpu_name=sys_info['gpu_name'], gpu_vendor=sys_info['gpu_vendor'],
                gpu_type=sys_info['gpu_type'], cpu_cores=sys_info['cpu_cores'],
                system_ram_gb=sys_info['system_ram_gb'])
        print(f"Logged {len(dets3)} detections")
    
    output4, duration4 = run_scanner("Run 4 - Full Tile", "run4_fulltile", overlap=0.0)
    if output4:
        kmz4 = output4 / "LAKE_MICHIGAN_SOUTH_CENSUS.kmz"
        dets4 = parse_kml(kmz4)
        log_run("Run 4 - Full Tile", dets4, chunking=False, overlap=0.0, duration=duration4,
                gpu_name=sys_info['gpu_name'], gpu_vendor=sys_info['gpu_vendor'],
                gpu_type=sys_info['gpu_type'], cpu_cores=sys_info['cpu_cores'],
                system_ram_gb=sys_info['system_ram_gb'])
        print(f"Logged {len(dets4)} detections")
    
    # Analyze
    analyze()
    
    print("\n" + "=" * 80)
    print("COMPLETE")
    print("=" * 80)
    print(f"\nDatabase: {DB_PATH}")
    print("\nQuery examples:")
    print("  SELECT run_name, detection_count FROM runs;")
    print("  SELECT COUNT(*) FROM detections WHERE run_id=1;")

if __name__ == "__main__":
    main()
