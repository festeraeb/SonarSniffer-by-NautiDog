#!/usr/bin/env python3
"""
REPEATABILITY TEST - NO SIMULATIONS
Run same real tile 5 times, verify identical results

RULES:
1. REAL DATA ONLY - no synthetic tests
2. LOG EVERYTHING - database records all runs
3. DEFINE PASS/FAIL BEFORE RUNNING
4. NO CHERRY-PICKING - accept whatever results show
"""

import json
import hashlib
import numpy as np
from pathlib import Path
from datetime import datetime
import subprocess
import sys

# ============================================================================
# CONFIGURATION - REAL DATA ONLY
# ============================================================================

# Real HLS tile from 2021 low water survey
TEST_TILE = "HLS.L30.T16TDN.2021182T162824.v2.0"
TEST_TILE_DIR = Path(r"C:\Users\thomf\programming\cesarops-wreckhunter build\wreckhunter2000\data\cache\census_raw\2021_low_water")

# Output directory for test results
OUTPUT_DIR = Path(r"C:\Users\thomf\programming\cesarops-wreckhunter build\outputs\repeatability_test")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Database for logging
DB_PATH = OUTPUT_DIR / "cesarops_runs.db"

# Number of repeat runs
NUM_CHUNKED_RUNS = 3  # Same as original: 512x512 tiles with 10% overlap (includes original run)
NUM_FULL_TILE_RUNS = 3  # Full 3660x3660, no chunking, no resizing, no manipulation

# Pass/Fail Criteria (defined BEFORE running)
MAX_POSITION_DRIFT_M = 5.0  # Same detection must be within 5m
MAX_SCORE_DRIFT = 0.01  # Score must be within ±0.01
MIN_DETECTION_COUNT_MATCH = True  # All runs must find same number of detections

# ============================================================================
# HELPER FUNCTIONS
# ============================================================================

def compute_file_hash(file_path):
    """Compute SHA256 hash of file contents"""
    sha256_hash = hashlib.sha256()
    with open(file_path, "rb") as f:
        for byte_block in iter(lambda: f.read(4096), b""):
            sha256_hash.update(byte_block)
    return sha256_hash.hexdigest()

def arrays_identical(arr1, arr2, tolerance=1e-10):
    """Check if two numpy arrays are identical within tolerance"""
    return np.allclose(arr1, arr2, rtol=tolerance, atol=tolerance, equal_nan=True)

def log_run(run_number, start_time, end_time, detections, gpu_temp, notes=""):
    """Log run results to database"""
    import sqlite3
    
    conn = sqlite3.connect(str(DB_PATH))
    cursor = conn.cursor()
    
    # Create table if not exists
    cursor.execute('''
        CREATE TABLE IF NOT EXISTS repeatability_runs (
            run_id INTEGER PRIMARY KEY AUTOINCREMENT,
            run_number INTEGER,
            tile_name TEXT,
            start_time TEXT,
            end_time TEXT,
            duration_seconds REAL,
            detection_count INTEGER,
            detections_hash TEXT,
            gpu_temperature_c REAL,
            notes TEXT,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )
    ''')
    
    # Compute hash of detections for quick comparison
    detections_json = json.dumps(detections, sort_keys=True)
    detections_hash = hashlib.sha256(detections_json.encode()).hexdigest()
    
    # Insert run record
    cursor.execute('''
        INSERT INTO repeatability_runs 
        (run_number, tile_name, start_time, end_time, duration_seconds, 
         detection_count, detections_hash, gpu_temperature_c, notes)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
    ''', (
        run_number,
        TEST_TILE,
        start_time.isoformat(),
        end_time.isoformat(),
        (end_time - start_time).total_seconds(),
        len(detections),
        detections_hash,
        gpu_temp,
        notes
    ))
    
    conn.commit()
    conn.close()
    
    return detections_hash

def execute_run(run_number, mode, tile_size, overlap):
    """Execute a single run with specified configuration"""
    print(f"[RUN {run_number}] Mode: {mode}, Tile: {tile_size}×{tile_size}, Overlap: {overlap*100:.0f}%")
    print("-" * 80)
    
    start_time = datetime.now()
    print(f"  Start time: {start_time.strftime('%Y-%m-%d %H:%M:%S')}")
    
    # Call Rust scanner with appropriate parameters
    # Chunked mode uses existing scanner, full-tile needs --no-chunk flag
    cmd = [
        sys.executable, "-m", "cesarops_search",
        "--tile", str(TEST_TILE_DIR / TEST_TILE),
        "--mode", mode,
        "--tile-size", str(tile_size),
        "--overlap", str(overlap),
        "--output", str(OUTPUT_DIR / f"run_{run_number}"),
        "--log-to-db", str(DB_PATH)
    ]
    
    print(f"  Command: {' '.join(cmd)}")
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=300)
        if result.returncode != 0:
            print(f"  ERROR: {result.stderr}")
            return None
    except subprocess.TimeoutExpired:
        print(f"  ERROR: Timeout after 5 minutes")
        return None
    except Exception as e:
        print(f"  ERROR: {e}")
        return None
    
    end_time = datetime.now()
    duration = (end_time - start_time).total_seconds()
    
    print(f"  Duration: {duration:.2f}s")
    print(f"  Output: {OUTPUT_DIR / f'run_{run_number}'}")
    print()
    
    return {
        'run_number': run_number,
        'mode': mode,
        'tile_size': tile_size,
        'overlap': overlap,
        'duration': duration,
        'timestamp': start_time.isoformat(),
        'output_dir': OUTPUT_DIR / f"run_{run_number}"
    }

def compare_group(runs):
    """Compare runs within a group and show differences"""
    for i in range(len(runs) - 1):
        for j in range(i + 1, len(runs)):
            issues = compare_runs(
                runs[i]['detections'],
                runs[j]['detections']
            )
            
            if issues:
                print(f"  Run {runs[i]['run_number']} vs Run {runs[j]['run_number']}:")
                for issue in issues[:5]:
                    print(f"    • {issue}")
                if len(issues) > 5:
                    print(f"    ... and {len(issues) - 5} more issues")
                print()

def compare_runs(run1_detections, run2_detections):
    """Compare two runs for repeatability"""
    issues = []
    
    # Check detection count
    if len(run1_detections) != len(run2_detections):
        issues.append(f"Different detection counts: {len(run1_detections)} vs {len(run2_detections)}")
        return issues
    
    # Sort both by position for comparison
    run1_sorted = sorted(run1_detections, key=lambda d: (d['pixel_row'], d['pixel_col']))
    run2_sorted = sorted(run2_detections, key=lambda d: (d['pixel_row'], d['pixel_col']))
    
    # Compare each detection
    for i, (det1, det2) in enumerate(zip(run1_sorted, run2_sorted)):
        # Position drift
        row_drift = abs(det1['pixel_row'] - det2['pixel_row'])
        col_drift = abs(det1['pixel_col'] - det2['pixel_col'])
        position_drift_m = np.sqrt(row_drift**2 + col_drift**2) * 30.0  # 30m per pixel
        
        if position_drift_m > MAX_POSITION_DRIFT_M:
            issues.append(f"Detection {i}: Position drift {position_drift_m:.1f}m (max: {MAX_POSITION_DRIFT_M}m)")
        
        # Score drift
        score_drift = abs(det1['base_score'] - det2['base_score'])
        if score_drift > MAX_SCORE_DRIFT:
            issues.append(f"Detection {i}: Score drift {score_drift:.4f} (max: {MAX_SCORE_DRIFT})")
    
    return issues

# ============================================================================
# MAIN TEST EXECUTION
# ============================================================================

def run_repeatability_test():
    """Execute repeatability test"""
    print("=" * 80)
    print("REPEATABILITY TEST - REAL DATA ONLY")
    print("=" * 80)
    print()
    print(f"Tile: {TEST_TILE}")
    print(f"Location: {TEST_TILE_DIR}")
    print()
    print("TEST SEQUENCE:")
    print(f"  • Runs 1-{NUM_CHUNKED_RUNS}: Original scan config (512×512, 10% overlap, stitching)")
    print(f"  • Runs {NUM_CHUNKED_RUNS+1}-{NUM_CHUNKED_RUNS+NUM_FULL_TILE_RUNS}: Full tile (3660×3660, NO manipulation)")
    print()
    print("WHAT THIS TESTS:")
    print(f"  • Runs 1-{NUM_CHUNKED_RUNS} consistency: Is our current chunked approach repeatable?")
    print(f"  • Runs {NUM_CHUNKED_RUNS+1}-{NUM_CHUNKED_RUNS+NUM_FULL_TILE_RUNS} consistency: Is full-tile repeatable?")
    print(f"  • Cross-compare: Does chunking produce same results as full-tile?")
    print()
    print("PASS/FAIL CRITERIA (defined before running):")
    print(f"  • Max position drift: {MAX_POSITION_DRIFT_M}m")
    print(f"  • Max score drift: {MAX_SCORE_DRIFT}")
    print(f"  • Detection count match: {'YES' if MIN_DETECTION_COUNT_MATCH else 'NO'}")
    print()
    print("=" * 80)
    print()
    
    # Verify real data exists
    b04_file = TEST_TILE_DIR / f"{TEST_TILE}.B04.tif"
    b05_file = TEST_TILE_DIR / f"{TEST_TILE}.B05.tif"
    
    if not b04_file.exists():
        print(f"❌ ERROR: Real data file not found: {b04_file}")
        print("ABORTING - no simulations will be used")
        return False
    
    if not b05_file.exists():
        print(f"❌ ERROR: Real data file not found: {b05_file}")
        print("ABORTING - no simulations will be used")
        return False
    
    print(f"✓ Verified real data exists:")
    print(f"  • {b04_file.name} ({b04_file.stat().st_size / 1e6:.1f} MB)")
    print(f"  • {b05_file.name} ({b05_file.stat().st_size / 1e6:.1f} MB)")
    print()
    
    all_runs = []
    
    # PHASE 1: Chunked runs (512x512 with 10% overlap)
    print("=" * 80)
    print(f"PHASE 1: CHUNKED PROCESSING ({NUM_CHUNKED_RUNS} runs)")
    print("=" * 80)
    print("Configuration: 512×512 tiles, 10% overlap, stitched output")
    print()
    
    for run_idx in range(NUM_CHUNKED_RUNS):
        run_result = execute_run(
            run_number=run_idx + 1,
            mode="chunked",
            tile_size=512,
            overlap=0.10
        )
        all_runs.append(run_result)
    
    # PHASE 2: Full tile runs (no chunking)
    print()
    print("=" * 80)
    print(f"PHASE 2: FULL TILE PROCESSING ({NUM_FULL_TILE_RUNS} runs)")
    print("=" * 80)
    print("Configuration: Full 3660×3660 tile, no chunking, no overlap")
    print()
    
    for run_idx in range(NUM_FULL_TILE_RUNS):
        run_result = execute_run(
            run_number=NUM_CHUNKED_RUNS + run_idx + 1,
            mode="full_tile",
            tile_size=3660,
            overlap=0.0
        )
        all_runs.append(run_result)
    
    # Compare all runs
    print()
    print("=" * 80)
    print("COMPARISON RESULTS")
    print("=" * 80)
    print()
    
    # Group by mode
    chunked_runs = [r for r in all_runs if r['mode'] == 'chunked']
    full_runs = [r for r in all_runs if r['mode'] == 'full_tile']
    
    # Check chunked consistency
    chunked_hashes = set(r['hash'] for r in chunked_runs)
    print(f"Chunked Runs (1-{NUM_CHUNKED_RUNS}):")
    if len(chunked_hashes) == 1:
        print(f"  ✓ IDENTICAL - All {NUM_CHUNKED_RUNS} runs produced same results")
    else:
        print(f"  ⚠ DIFFERENT - {len(chunked_hashes)} unique outcomes")
        compare_group(chunked_runs)
    print()
    
    # Check full tile consistency
    full_hashes = set(r['hash'] for r in full_runs)
    print(f"Full Tile Runs ({NUM_CHUNKED_RUNS+1}-{NUM_CHUNKED_RUNS+NUM_FULL_TILE_RUNS}):")
    if len(full_hashes) == 1:
        print(f"  ✓ IDENTICAL - All {NUM_FULL_TILE_RUNS} runs produced same results")
    else:
        print(f"  ⚠ DIFFERENT - {len(full_hashes)} unique outcomes")
        compare_group(full_runs)
    print()
    
    # Cross-compare chunked vs full tile
    print("Cross-Comparison (Chunked vs Full Tile):")
    chunked_detections = chunked_runs[0]['detections']
    full_detections = full_runs[0]['detections']
    
    if len(chunked_detections) == len(full_detections):
        print(f"  ✓ Same detection count: {len(chunked_detections)}")
        
        # Compare positions
        issues = compare_runs(chunked_detections, full_detections)
        if not issues:
            print(f"  ✓ Same detections (position + score)")
            print(f"  ✅ PASS - Chunking produces identical results to full tile")
            return True
        else:
            print(f"  ⚠ Detection differences found:")
            for issue in issues[:10]:
                print(f"    • {issue}")
            if len(issues) > 10:
                print(f"    ... and {len(issues) - 10} more")
            print(f"  ❌ FAIL - Chunking introduces artifacts")
            return False
    else:
        print(f"  ⚠ Different detection counts:")
        print(f"    • Chunked: {len(chunked_detections)}")
        print(f"    • Full Tile: {len(full_detections)}")
        print(f"    • Difference: {abs(len(chunked_detections) - len(full_detections))}")
        print(f"  ❌ FAIL - Chunking misses or adds detections")
        return False

if __name__ == "__main__":
    success = run_repeatability_test()
    sys.exit(0 if success else 1)
