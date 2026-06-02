#!/usr/bin/env python3
"""
GPU Stress Test - Process large real TIFFs and show timing
"""

import subprocess
import time
from pathlib import Path

def test_gpu_processing():
    """Test GPU with real large TIFFs and show timing"""
    
    # Find large real TIFFs
    test_files = [
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache\S2C_16TDN_20250916_0_L2A.B11.tif"),
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache\S2C_16TDN_20250916_0_L2A.B12.tif"),
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache\S2C_16TDN_20250916_0_L2A.B08.tif"),
    ]
    
    exe = Path("target/release/cesarops-gpu.exe")
    
    print("=" * 80)
    print("M2200 GPU STRESS TEST")
    print("=" * 80)
    print()
    print("This will process large TIFFs (5490x5490 = 30 million pixels each)")
    print("Watch GPU usage in Task Manager > Performance > GPU")
    print()
    
    for i, tiff in enumerate(test_files, 1):
        if not tiff.exists():
            print(f"[{i}] SKIP: {tiff.name} (not found)")
            continue
        
        print(f"[{i}] Processing: {tiff.name}")
        print(f"    Size: 5490x5490 pixels (30,131,886 pixels)")
        print(f"    Starting GPU processing...")
        
        start = time.time()
        
        result = subprocess.run(
            [str(exe), str(tiff), "--threshold", "2.0"],
            capture_output=True,
            text=True
        )
        
        elapsed = time.time() - start
        
        if result.returncode == 0:
            # Parse output
            if "GPU processing complete" in result.stdout:
                print(f"    ✓ GPU Processing Time: {elapsed:.2f} seconds")
                print(f"    ✓ Throughput: {30131886/elapsed:,.0f} pixels/second")
                
                # Check for M2200
                if "Quadro M2200" in result.stdout and "is active" in result.stdout:
                    print(f"    ✓ Confirmed: M2200 CUDA cores used")
                
                # Count anomalies
                for line in result.stdout.split('\n'):
                    if "Detected" in line and "anomalies" in line:
                        print(f"    {line.strip()}")
            else:
                print(f"    ✗ Processing failed")
        else:
            print(f"    ✗ Error: {result.stderr[:100]}")
        
        print()
    
    print("=" * 80)
    print("STRESS TEST COMPLETE")
    print("=" * 80)
    print()
    print("If processing took 1-3 seconds per file, the M2200 is working.")
    print("If it was instant (<0.1s), the GPU didn't actually process.")

if __name__ == "__main__":
    test_gpu_processing()
