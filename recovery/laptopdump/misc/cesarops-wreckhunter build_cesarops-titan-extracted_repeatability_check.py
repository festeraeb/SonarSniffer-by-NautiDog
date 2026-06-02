#!/usr/bin/env python3
"""
REPEATABILITY CHECK - DATABASE LOGGING
Run scanner twice, log results to database, compare
"""

import sqlite3
import json
import subprocess
import sys
from pathlib import Path
from datetime import datetime

# ============================================================================
# CONFIGURATION
# ============================================================================

DATA_DIR = Path(r".\wreckhunter2000\data\cache\census_raw\2021_low_water")
OUTPUT_BASE = Path(r".\outputs\repeatability_db")
DB_PATH = OUTPUT_BASE / "cesarops_runs.db"

OUTPUT_BASE.mkdir(parents=True, exist_ok=True)

# ============================================================================
# DATABASE SETUP
# ============================================================================

def init_db():
    """Initialize database schema"""
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    cursor.execute('''
        CREATE TABLE IF NOT EXISTS scan_runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_number INTEGER,
            mode TEXT,
            tile_size TEXT,
            overlap REAL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
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
            grid_ref TEXT,
            score REAL,
            classification TEXT,
            aluminum_ratio REAL,
            thermal_delta REAL,
            pixel_row INTEGER,
            pixel_col INTEGER,
            FOREIGN KEY (run_id) REFERENCES scan_runs(run_id)
        )
    ''')
    
    conn.commit()
    conn.close()

def log_run(run_number, mode, tile_size, overlap, detections):
    """Log run results to database"""
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    # Insert run metadata
    cursor.execute('''
        INSERT INTO scan_runs (run_number, mode, tile_size, overlap)
        VALUES (?, ?, ?, ?)
    ''', (run_number, mode, tile_size, overlap))
    
    run_id = cursor.lastrowid
    
    # Insert detections
    for det in detections:
        cursor.execute('''
            INSERT INTO detections (
                run_id, utm_easting, utm_northing, wgs84_lat, wgs84_lon,
                grid_ref, score, classification, aluminum_ratio, thermal_delta,
                pixel_row, pixel_col
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        ''', (
            run_id,
            det.get('utm_easting', 0),
            det.get('utm_northing', 0),
            det.get('wgs84_lat', 0),
            det.get('wgs84_lon', 0),
            det.get('grid_ref', ''),
            det.get('score', 0),
            det.get('classification', ''),
            det.get('aluminum_ratio', 0),
            det.get('thermal_delta', 0),
            det.get('pixel_row', 0),
            det.get('pixel_col', 0),
        ))
    
    conn.commit()
    conn.close()
    
    return run_id

# ============================================================================
# RUN SCANNER
# ============================================================================

def run_scanner(run_number, mode, tile_size, overlap):
    """Run scanner and return output directory"""
    output_dir = OUTPUT_BASE / f"run_{run_number}_{mode}"
    output_dir.mkdir(parents=True, exist_ok=True)
    
    print(f"[RUN {run_number}] Mode: {mode}, Tile: {tile_size}, Overlap: {overlap}")
    print("-" * 80)
    
    cmd = [
        r".\target\release\cesarops-search.exe",
        "scan", "michigan",
        "--input", str(DATA_DIR),
        "--output", str(output_dir),
        "--min-confidence", "0.5",
        "--overlap", str(overlap)
    ]
    
    print(f"Command: {' '.join(cmd)}")
    print()
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    
    if result.returncode != 0:
        print(f"ERROR: {result.stderr}")
        return None
    
    return output_dir

# ============================================================================
# PARSE OUTPUT
# ============================================================================

def parse_kml(kml_path):
    """Parse KML file and extract detections"""
    import re
    
    detections = []
    
    if not kml_path.exists():
        print(f"WARNING: KML not found: {kml_path}")
        return detections
    
    content = kml_path.read_text()
    
    # Simple regex parsing - extract coordinates and descriptions
    # This is a placeholder - would need proper KML parsing for production
    placemarks = re.findall(r'<Placemark>(.*?)</Placemark>', content, re.DOTALL)
    
    for pm in placemarks:
        det = {}
        
        # Extract coordinates
        coords_match = re.search(r'<coordinates>([^<]+)</coordinates>', pm)
        if coords_match:
            coords = coords_match.group(1).strip().split(',')
            if len(coords) >= 2:
                det['wgs84_lon'] = float(coords[0])
                det['wgs84_lat'] = float(coords[1])
        
        # Extract UTM from description
        utm_e_match = re.search(r'E: ([\d.]+)m', pm)
        utm_n_match = re.search(r'N: ([\d.]+)m', pm)
        if utm_e_match:
            det['utm_easting'] = float(utm_e_match.group(1))
        if utm_n_match:
            det['utm_northing'] = float(utm_n_match.group(1))
        
        # Extract score
        score_match = re.search(r'<b>Score:</b></td><td>([\d.]+)', pm)
        if score_match:
            det['score'] = float(score_match.group(1))
        
        # Extract classification
        class_match = re.search(r'<b>Classification:</b></td><td>([^<]+)', pm)
        if class_match:
            det['classification'] = class_match.group(1).strip()
        
        # Extract aluminum ratio
        alum_match = re.search(r'<b>B08/B04 Ratio:</b></td><td>([\d.]+)', pm)
        if alum_match:
            det['aluminum_ratio'] = float(alum_match.group(1))
        
        # Extract thermal delta
        therm_match = re.search(r'<b>Thermal Delta:</b></td><td>([\d.]+)', pm)
        if therm_match:
            det['thermal_delta'] = float(therm_match.group(1))
        
        # Extract pixel position
        row_match = re.search(r'Row: (\d+), Col: (\d+)', pm)
        if row_match:
            det['pixel_row'] = int(row_match.group(1))
            det['pixel_col'] = int(row_match.group(2))
        
        # Extract grid ref
        grid_match = re.search(r'<b>Grid Ref:</b></td><td>([^<]+)', pm)
        if grid_match:
            det['grid_ref'] = grid_match.group(1).strip()
        
        detections.append(det)
    
    return detections

# ============================================================================
# ANALYZE
# ============================================================================

def analyze():
    """Analyze database for repeatability"""
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    print("=" * 80)
    print("ANALYSIS RESULTS")
    print("=" * 80)
    print()
    
    # Detection counts by run
    cursor.execute('''
        SELECT r.run_number, r.mode, COUNT(d.detection_id) as detection_count
        FROM scan_runs r
        LEFT JOIN detections d ON r.run_id = d.run_id
        GROUP BY r.run_id
        ORDER BY r.run_number
    ''')
    
    print("Detection Counts:")
    for row in cursor.fetchall():
        print(f"  Run {row[0]} ({row[1]}): {row[2]} detections")
    
    print()
    
    # Compare first 5 detections between runs
    cursor.execute('''
        SELECT d1.utm_easting, d1.utm_northing, d2.utm_easting, d2.utm_northing
        FROM detections d1
        JOIN detections d2 ON d1.detection_id = d2.detection_id
        WHERE d1.run_id = 1 AND d2.run_id = 2
        LIMIT 5
    ''')
    
    print("Position Comparison (First 5 detections, Run 1 vs Run 2):")
    print("  Run 1 (E, N)                    | Run 2 (E, N)")
    print("  " + "-" * 70)
    for row in cursor.fetchall():
        print(f"  {row[0]:12.2f}, {row[1]:12.2f}  |  {row[2]:12.2f}, {row[3]:12.2f}")
    
    print()
    
    # Check for identical detections
    cursor.execute('''
        SELECT COUNT(*)
        FROM detections d1
        JOIN detections d2 ON 
            d1.utm_easting = d2.utm_easting AND
            d1.utm_northing = d2.utm_northing AND
            d1.score = d2.score
        WHERE d1.run_id = 1 AND d2.run_id = 2
    ''')
    
    identical_count = cursor.fetchone()[0]
    print(f"Identical detections (same position + score): {identical_count}")
    
    conn.close()

# ============================================================================
# MAIN
# ============================================================================

def main():
    print("=" * 80)
    print("REPEATABILITY CHECK - DATABASE LOGGING")
    print("=" * 80)
    print()
    
    # Initialize database
    init_db()
    print("✓ Database initialized")
    print()
    
    # Run 1-3: Chunked (512x512, 10% overlap)
    for i in range(1, 4):
        output_dir = run_scanner(i, "chunked", "512", 0.10)
        if output_dir:
            kml_path = output_dir / "LAKE_MICHIGAN_SOUTH_CENSUS.kml"
            detections = parse_kml(kml_path)
            log_run(i, "chunked", "512", 0.10, detections)
            print(f"Logged {len(detections)} detections to database")
        print()
    
    # Run 4-6: Full tile (3660x3660, no chunking)
    for i in range(4, 7):
        output_dir = run_scanner(i, "full_tile", "3660", 0.0)
        if output_dir:
            kml_path = output_dir / "LAKE_MICHIGAN_SOUTH_CENSUS.kml"
            detections = parse_kml(kml_path)
            log_run(i, "full_tile", "3660", 0.0, detections)
            print(f"Logged {len(detections)} detections to database")
        print()
    
    # Analyze
    analyze()
    
    print()
    print("=" * 80)
    print("COMPLETE")
    print("=" * 80)
    print()
    print(f"Database: {DB_PATH}")
    print()
    print("Next: Run SQL queries to analyze repeatability")
    print()

if __name__ == "__main__":
    main()
