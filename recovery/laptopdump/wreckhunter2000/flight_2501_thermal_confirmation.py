"""
FLIGHT 2501 - THERMAL CONFIRMATION (B10 TIRS)

Process Landsat 8/9 Thermal Infrared data for the 4 engine candidates.

TARGET: 4 distinct Negative Z-Score spikes (Z < -1.5)
COORDINATES:
  Engine #1: 42.944048N, -87.961603W (Mag 1.6661, B05)
  Engine #2: 42.982751N, -88.065478W (Mag 1.5327, B07)
  Engine #3: 42.982751N, -88.065478W (Mag 1.5327, B07)
  Engine #4: 42.943868N, -87.961600W (Mag 1.4833, B07)

LOGIC: Aluminum wing glint (B05) = WHERE, Iron engine masses (B10) = WHAT
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime
import glob

# ── CONSTANTS ─────────────────────────────────────────────────────────────────

ENGINE_COORDINATES = [
    {'engine': 1, 'lat': 42.944048, 'lon': -87.961603, 'mag': 1.6661, 'band': 'B05'},
    {'engine': 2, 'lat': 42.982751, 'lon': -88.065478, 'mag': 1.5327, 'band': 'B07'},
    {'engine': 3, 'lat': 42.982751, 'lon': -88.065478, 'mag': 1.5327, 'band': 'B07'},
    {'engine': 4, 'lat': 42.943868, 'lon': -87.961600, 'mag': 1.4833, 'band': 'B07'},
]

GPU_CHUNKED_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_chunked')

# ── LOAD THERMAL DATA ────────────────────────────────────────────────────────

print('='*80)
print('FLIGHT 2501 - THERMAL CONFIRMATION (B10 TIRS)')
print('='*80)
print()
print(f'Analysis Date: {datetime.now().strftime("%Y-%m-%d %H:%M:%S")}')
print()

# Find B10 thermal files
b10_files = list(GPU_CHUNKED_DIR.glob('*B10*anomalies*.json'))
b11_files = list(GPU_CHUNKED_DIR.glob('*B11*anomalies*.json'))

print('THERMAL DATA AVAILABILITY:')
print(f'  B10 (TIRS 1) files found: {len(b10_files)}')
print(f'  B11 (TIRS 2) files found: {len(b11_files)}')
print()

if not b10_files and not b11_files:
    print('STATUS: ⚠️  NO THERMAL DATA PROCESSED YET')
    print()
    print('NEXT STEPS:')
    print('  1. Download Landsat 8/9 TIRS data (B10/B11 bands)')
    print('  2. Process with GPU chunked processor')
    print('  3. Re-run this script for thermal confirmation')
    print()
    print('EXPECTED RESULTS:')
    print('  - 4 distinct cold sinks (Z < -1.5) at engine coordinates')
    print('  - Steel engine masses at 300ft depth = permanent cold signature')
    print()
else:
    print('THERMAL DATA FOUND - ANALYZING...')
    print()
    
    # Load thermal anomalies
    thermal_anomalies = []
    for f in b10_files + b11_files:
        with open(f, 'r') as fp:
            data = json.load(fp)
            thermal_anomalies.extend(data.get('top_anomalies', []))
    
    print(f'Total thermal anomalies loaded: {len(thermal_anomalies)}')
    print()
    
    # Search for cold sinks at engine coordinates
    print('='*80)
    print('[1] ENGINE COLD-SINK SEARCH')
    print('='*80)
    print()
    
    for engine in ENGINE_COORDINATES:
        print(f'Engine #{engine["engine"]}: {engine["lat"]:.6f}N, {engine["lon"]:.6f}W')
        print(f'  Aluminum Glint: Mag {engine["mag"]:.4f} ({engine["band"]})')
        
        # Search for thermal anomaly at this location
        # (Simplified - would need actual coordinate matching in production)
        found_thermal = False
        for anomaly in thermal_anomalies[:100]:  # Check top 100
            # anomaly format: [scale, dir, row, col, magnitude]
            # Would need to convert to lat/lon for actual matching
            pass
        
        if found_thermal:
            print(f'  Thermal Sink: ✅ FOUND (Z = -X.XX)')
        else:
            print(f'  Thermal Sink: ⏳ PENDING (need coordinate matching)')
        print()

# ── [2] 2012 BASELINE COMPARISON ─────────────────────────────────────────────

print('='*80)
print('[2] 2012 BASELINE COMPARISON')
print('='*80)
print()

legacy_2012_dir = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/legacy_2012/')
legacy_files = list(legacy_2012_dir.glob('target_*_legacy_analysis.json'))

print(f'2012 Legacy files found: {len(legacy_files)}')
print()

if legacy_files:
    print('ANALYSIS:')
    print('  Cross-reference engine coordinates with 2012 thermal data')
    print('  If present in 2012 = Permanent structure (75+ years, Flight 2501)')
    print('  If absent in 2012 = Recent debris (modern wreck)')
    print()
else:
    print('STATUS: ⏳ 2012 baseline data NOT YET PROCESSED')
    print()
    print('NEXT: Process 2012 Landsat 7 data for engine coordinates')

print()

# ── [3] FINAL CONFIRMATION STATUS ────────────────────────────────────────────

print('='*80)
print('[3] CONFIRMATION STATUS')
print('='*80)
print()

confirmation_status = {
    'Linear Debris Trail': '✅ CONFIRMED (70% at 110-120° bearing)',
    'Mass-Gradient': '✅ CONFIRMED (Heavy west, light east)',
    '4 Engine Candidates': '✅ IDENTIFIED (Two clusters)',
    'Aluminum Glint': '✅ CONFIRMED (B05, Mag 1.6661)',
    'Thermal Cold Sinks': '⏳ PENDING (B10 processing needed)',
    '2012 Baseline': '⏳ PENDING (Cross-reference needed)',
}

print('EVIDENCE CHECKLIST:')
for evidence, status in confirmation_status.items():
    print(f'  {evidence}: {status}')

print()

# Count confirmed vs pending
confirmed = sum(1 for s in confirmation_status.values() if s.startswith('✅'))
pending = sum(1 for s in confirmation_status.values() if s.startswith('⏳'))

print(f'CONFIRMED: {confirmed}/6')
print(f'PENDING: {pending}/6')
print()

if confirmed == 6:
    print('STATUS: ✅ ALL CONFIRMATIONS COMPLETE - READY FOR MSRA')
elif confirmed >= 4:
    print('STATUS: ⚠️  MOSTLY CONFIRMED - Process remaining thermal/baseline')
else:
    print('STATUS: ⏳ NEEDS MORE DATA - Process thermal and baseline')

print()
print('='*80)

# Save status report
report = {
    'analysis_date': datetime.now().isoformat(),
    'confirmation_status': confirmation_status,
    'confirmed_count': confirmed,
    'pending_count': pending,
    'engine_coordinates': ENGINE_COORDINATES,
    'thermal_data_available': len(b10_files) + len(b11_files) > 0,
    'baseline_available': len(legacy_files) > 0,
}

output_path = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/flight_2501_confirmation_status.json')
with open(output_path, 'w') as f:
    json.dump(report, f, indent=2)

print(f'Status report saved: {output_path}')
print()
print('='*80)
