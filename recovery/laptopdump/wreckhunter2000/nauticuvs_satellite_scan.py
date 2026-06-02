"""
nauticuvs_satellite_scan.py

Process Sentinel-2 satellite imagery with REAL Nauticuvs curvelets
and save outputs for analysis.
"""

import bag_processor_rs
import rasterio
import numpy as np
import json
import time
from pathlib import Path
from datetime import datetime

# Output directory
OUTPUT_DIR = Path('c:/Users/thomf/programming/wreckhunter2000/outputs/nauticuvs_scans')
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Input cache
CACHE_DIR = Path('c:/Users/thomf/programming/Bagrecovery/outputs/rossa_forensic_cache')

def process_sentinel2_with_nauticuvs():
    """Process all downloaded Sentinel-2 bands with Nauticuvs curvelets."""
    
    print('='*70)
    print('NAUTICUVS CURVELETS - SENTINEL-2 PROCESSING')
    print('='*70)
    print()
    
    # Find all TIFF files
    tif_files = list(CACHE_DIR.glob('*.tif'))
    print(f'Found {len(tif_files)} Sentinel-2 bands to process')
    print()
    
    results = []
    
    for i, tif_path in enumerate(tif_files, 1):
        band_name = tif_path.stem.split('.')[-1]  # Extract band name (B01, B02, etc.)
        print(f'[{i}/{len(tif_files)}] Processing {tif_path.name}...')
        
        try:
            with rasterio.open(tif_path) as src:
                height, width = src.shape
                total_pixels = height * width
                
                print(f'  Size: {height}x{width} ({total_pixels/1e6:.1f}M pixels)')
                
                # Process in chunks to avoid memory issues
                chunk_size = 2000
                start = time.time()
                chunks_processed = 0
                total_coeffs = []
                
                for y in range(0, height, chunk_size):
                    for x in range(0, width, chunk_size):
                        # Read chunk
                        chunk = src.read(1, window=(
                            (y, min(y+chunk_size, height)),
                            (x, min(x+chunk_size, width))
                        )).astype(np.float32)
                        h, w = chunk.shape
                        
                        # Apply Nauticuvs curvelets
                        result = bag_processor_rs.apply_curvelets_filter(
                            chunk.flatten().tolist(), w, h, 4
                        )
                        total_coeffs.extend(result)
                        chunks_processed += 1
                
                elapsed = time.time() - start
                
                # Convert to numpy array
                coeffs_array = np.array(total_coeffs).reshape(height, width)
                
                # Save processed output
                output_path = OUTPUT_DIR / f'{tif_path.stem}_curvelets.npy'
                np.save(output_path, coeffs_array)
                
                # Calculate statistics
                mean_val = float(coeffs_array.mean())
                std_val = float(coeffs_array.std())
                max_val = float(coeffs_array.max())
                min_val = float(coeffs_array.min())
                
                print(f'  ✓ Processed in {elapsed:.2f}s ({total_pixels/elapsed/1e6:.2f}M pixels/sec)')
                print(f'  ✓ Saved to: {output_path.name}')
                print(f'  ✓ Stats: mean={mean_val:.4f}, std={std_val:.4f}, range=[{min_val:.4f}, {max_val:.4f}]')
                print()
                
                results.append({
                    'band': band_name,
                    'input_file': tif_path.name,
                    'output_file': output_path.name,
                    'size': [height, width],
                    'processing_time_sec': elapsed,
                    'throughput_mpix_sec': total_pixels/elapsed/1e6,
                    'statistics': {
                        'mean': mean_val,
                        'std': std_val,
                        'min': min_val,
                        'max': max_val,
                    }
                })
                
        except Exception as e:
            print(f'  ✗ Error: {e}')
            print()
            results.append({
                'band': band_name,
                'input_file': tif_path.name,
                'error': str(e),
            })
    
    # Save summary JSON
    summary_path = OUTPUT_DIR / 'nauticuvs_processing_summary.json'
    with open(summary_path, 'w') as f:
        json.dump({
            'processed_at': datetime.now().isoformat(),
            'total_bands': len(tif_files),
            'successful': len([r for r in results if 'error' not in r]),
            'failed': len([r for r in results if 'error' in r]),
            'results': results,
        }, f, indent=2)
    
    print('='*70)
    print('PROCESSING COMPLETE')
    print('='*70)
    print(f'Total bands processed: {len(results)}')
    print(f'Successful: {len([r for r in results if "error" not in r])}')
    print(f'Failed: {len([r for r in results if "error" in r])}')
    print(f'Outputs saved to: {OUTPUT_DIR}')
    print(f'Summary: {summary_path}')
    print('='*70)
    
    return results

if __name__ == '__main__':
    process_sentinel2_with_nauticuvs()
