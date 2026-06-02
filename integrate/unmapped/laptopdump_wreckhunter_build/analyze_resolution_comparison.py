#!/usr/bin/env python3
"""
RESOLUTION COMPARISON TEST
Run same tiles at full resolution and 512x512, compare accuracy and speed

Post-processing only - no scanner code changes needed
"""

import sqlite3
import json
from pathlib import Path
from datetime import datetime

# ============================================================================
# CONFIGURATION
# ============================================================================

DB_PATH = Path(r".\outputs\run_zero\cesarops_runs.db")
OUTPUT_DIR = Path(r".\outputs\resolution_comparison")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# ============================================================================
# ANALYSIS
# ============================================================================

def analyze_resolution_comparison():
    """Compare full resolution vs 512x512 runs"""
    
    conn = sqlite3.connect(str(DB_PATH))
    c = conn.cursor()
    
    print("=" * 80)
    print("RESOLUTION COMPARISON ANALYSIS")
    print("=" * 80)
    print()
    
    # Get runs by mode
    c.execute('''
        SELECT run_id, run_name, detection_count, duration_seconds, chunking_enabled
        FROM runs
        ORDER BY run_id
    ''')
    
    runs = c.fetchall()
    
    print("RUN SUMMARY:")
    print("-" * 80)
    full_res_runs = []
    reduced_res_runs = []
    
    for run in runs:
        mode = "Full (3660x3660)" if run[4] else "Reduced (512x512)"
        print(f"Run {run[0]}: {run[1]:<30} | {mode:<20} | {run[2]:>5} detections | {run[3]:>5.2f}s")
        
        if run[4]:
            full_res_runs.append(run)
        else:
            reduced_res_runs.append(run)
    
    print()
    print("COMPARISON METRICS:")
    print("-" * 80)
    
    # Compare detection counts
    if full_res_runs and reduced_res_runs:
        full_avg = sum(r[2] for r in full_res_runs) / len(full_res_runs)
        reduced_avg = sum(r[2] for r in reduced_res_runs) / len(reduced_res_runs)
        
        print(f"Average Detections:")
        print(f"  Full Resolution:     {full_avg:.0f}")
        print(f"  Reduced Resolution:  {reduced_avg:.0f}")
        print(f"  Difference:          {full_avg - reduced_avg:+.0f} ({(full_avg - reduced_avg)/full_avg*100:+.1f}%)")
        print()
        
        # Compare runtime
        full_time = sum(r[3] for r in full_res_runs) / len(full_res_runs)
        reduced_time = sum(r[3] for r in reduced_res_runs) / len(reduced_res_runs)
        
        print(f"Average Runtime:")
        print(f"  Full Resolution:     {full_time:.2f}s")
        print(f"  Reduced Resolution:  {reduced_time:.2f}s")
        print(f"  Speedup:             {full_time/reduced_time:.2f}x faster")
        print()
        
        # Accuracy comparison (position drift)
        c.execute('''
            SELECT 
                d1.pixel_row, d1.pixel_col, d1.score,
                d2.pixel_row, d2.pixel_col, d2.score
            FROM detections d1
            JOIN detections d2 ON d1.run_id = ? AND d2.run_id = ?
            WHERE d1.pixel_row = d2.pixel_row AND d1.pixel_col = d2.pixel_col
            LIMIT 100
        ''', (full_res_runs[0][0], reduced_res_runs[0][0]))
        
        matches = c.fetchall()
        
        if matches:
            position_match_rate = len(matches) / min(full_avg, reduced_avg) * 100
            print(f"Position Match Rate: {position_match_rate:.1f}%")
            print(f"  (Detections at exact same pixel location)")
            print()
            
            # Score correlation
            score_diffs = [abs(m[2] - m[5]) for m in matches]
            avg_score_diff = sum(score_diffs) / len(score_diffs)
            print(f"Average Score Difference: {avg_score_diff:.3f}")
            print(f"  (Lower = better correlation)")
    
    conn.close()
    
    print()
    print("=" * 80)
    print("EXPORTING RESULTS")
    print("=" * 80)
    
    # Export to JSON
    results = {
        'timestamp': datetime.now().isoformat(),
        'full_resolution_runs': len(full_res_runs),
        'reduced_resolution_runs': len(reduced_res_runs),
        'metrics': {
            'full_avg_detections': full_avg if full_res_runs else 0,
            'reduced_avg_detections': reduced_avg if reduced_res_runs else 0,
            'full_avg_time': full_time if full_res_runs else 0,
            'reduced_avg_time': reduced_time if reduced_res_runs else 0,
            'speedup': full_time/reduced_time if (full_res_runs and reduced_res_runs) else 0,
        }
    }
    
    output_file = OUTPUT_DIR / "resolution_comparison_results.json"
    with open(output_file, 'w') as f:
        json.dump(results, f, indent=2)
    
    print(f"Results exported to: {output_file}")

if __name__ == "__main__":
    analyze_resolution_comparison()
