"""
gpu_batch_processor.py

Queues ALL Sentinel-2 TIFF files for full GPU processing.
Processes one file at a time with thermal breaks between files.

Automatically processes all bands in:
  c:/Users/thomf/programming/Bagrecovery/outputs/rossa_forensic_cache/

Output to:
  c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_full/
"""

import subprocess
import time
from pathlib import Path
from datetime import datetime

# ── Configuration ─────────────────────────────────────────────────────────────

INPUT_DIR = Path('c:/Users/thomf/programming/Bagrecovery/outputs/rossa_forensic_cache')
OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/gpu_full')
PROCESSOR_SCRIPT = Path('c:/Users/thomf/programming/wreckhunter2000/gpu_full_image_processor.py')

# Thermal management between files
BREAK_BETWEEN_FILES = 180  # 3 minutes between files (GPU cooldown)

# ── Batch Processing ──────────────────────────────────────────────────────────

def main():
    """Process all Sentinel-2 TIFF files in queue."""
    
    print('='*70)
    print('GPU BATCH PROCESSOR - ALL SENTINEL-2 BANDS')
    print('='*70)
    print()
    
    # Find all TIFF files
    tiff_files = sorted(INPUT_DIR.glob('*.tif'))
    
    if not tiff_files:
        print(f'[!] No TIFF files found in {INPUT_DIR}')
        return
    
    print(f'Found {len(tiff_files)} TIFF files to process:')
    for i, tif in enumerate(tiff_files, 1):
        print(f'  {i}. {tif.name}')
    print()
    
    OUTPUT_DIR.mkdir(parents=True, exist_ok=True)
    
    # Process each file
    for i, tif_path in enumerate(tiff_files, 1):
        print('='*70)
        print(f'[{i}/{len(tiff_files)}] Processing {tif_path.name}...')
        print('='*70)
        
        # Run GPU processor
        cmd = [
            'C:\\Users\\thomf\\miniconda3\\Scripts\\conda.exe',
            'run', '-n', 'wreckhunter',
            'python', str(PROCESSOR_SCRIPT),
            '--input', str(tif_path),
            '--output', str(OUTPUT_DIR),
        ]
        
        print(f'Command: {" ".join(cmd)}')
        print()
        
        try:
            result = subprocess.run(cmd, timeout=1800)  # 30 minute timeout per file
            
            if result.returncode == 0:
                print(f'✓ {tif_path.name} complete')
            else:
                print(f'✗ {tif_path.name} failed (exit code {result.returncode})')
        
        except subprocess.TimeoutExpired:
            print(f'✗ {tif_path.name} timed out (30 min limit)')
        except Exception as e:
            print(f'✗ {tif_path.name} error: {e}')
        
        print()
        
        # Thermal break between files (except last one)
        if i < len(tiff_files):
            print(f'[THERMAL BREAK] Cooling down for {BREAK_BETWEEN_FILES//60} minutes...')
            time.sleep(BREAK_BETWEEN_FILES)
            print(f'[RESUME] GPU cooled, continuing to next file...\n')
    
    print('='*70)
    print('BATCH PROCESSING COMPLETE')
    print('='*70)
    print(f'Processed: {len(tiff_files)} files')
    print(f'Output: {OUTPUT_DIR}')
    print()
    print('Next: Check anomaly results in output directory!')
    print('='*70)


if __name__ == '__main__':
    main()
