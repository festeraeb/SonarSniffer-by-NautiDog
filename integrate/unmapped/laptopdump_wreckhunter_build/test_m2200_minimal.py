#!/usr/bin/env python3
"""
Minimal M2200 Test - Small synthetic data to force GPU compute
"""

import numpy as np
import subprocess
from pathlib import Path
from PIL import Image
import time

def create_small_test_tiff():
    """Create tiny TIFF with strong anomalies"""
    # Small 100x100 image
    width, height = 100, 100
    
    # Base temperature
    thermal_data = np.full((height, width), 285.0, dtype=np.float32)
    
    # Add 5 STRONG cold anomalies (50K below ambient)
    anomalies = [
        (25, 25), (25, 75), (50, 50), (75, 25), (75, 75)
    ]
    
    for y, x in anomalies:
        for dy in range(-3, 4):
            for dx in range(-3, 4):
                if 0 <= y+dy < height and 0 <= x+dx < width:
                    thermal_data[y+dy, x+dx] = 235.0  # 50K colder
    
    # Convert to uint16
    thermal_u16 = ((thermal_data - 200) * 200).clip(0, 65535).astype(np.uint16)
    
    # Save
    output_path = Path("small_test.tif")
    img = Image.fromarray(thermal_u16, mode='I;16')
    img.save(output_path)
    
    print(f"Created: {output_path}")
    print(f"  Size: {width}x{height} = {width*height:,} pixels")
    print(f"  Anomalies: {len(anomalies)} strong cold spots")
    
    return output_path

def run_gpu_test(tiff_path):
    """Run GPU test and parse output"""
    exe = Path("target/release/cesarops-gpu.exe")
    
    if not exe.exists():
        print("ERROR: GPU engine not built")
        print("Run: cargo build --release --bin cesarops-gpu")
        return False
    
    print("\n" + "="*80)
    print("RUNNING M2200 GPU TEST")
    print("="*80)
    print()
    
    start = time.time()
    result = subprocess.run(
        [str(exe), str(tiff_path), "--threshold", "1.5"],
        capture_output=True,
        text=True
    )
    elapsed = time.time() - start
    
    print(result.stdout)
    
    if result.returncode != 0:
        print("ERROR:", result.stderr)
        return False
    
    # Parse output
    m2200_active = False
    intel_detected = False
    anomalies_found = 0
    
    for line in result.stdout.split('\n'):
        if "Quadro M2200" in line and "is active" in line:
            m2200_active = True
        if "Intel" in line and "Evaluating" in line:
            intel_detected = True
        if "Detected" in line and "anomalies" in line:
            try:
                anomalies_found = int(line.split()[1])
            except:
                pass
    
    print("\n" + "="*80)
    print("TEST RESULTS")
    print("="*80)
    print(f"Processing Time: {elapsed:.3f} seconds")
    print(f"M2200 Active: {'YES' if m2200_active else 'NO'}") 
    print(f"Intel GPU Detected: {'YES' if intel_detected else 'NO'}")
    print(f"Anomalies Found: {anomalies_found}")
    print()
    
    if m2200_active and anomalies_found > 0:
        print("SUCCESS: M2200 is processing and detecting anomalies!")
        return True
    elif m2200_active and anomalies_found == 0:
        print("PARTIAL: M2200 active but no anomalies detected")
        print("  -> Threshold may be too high or data issue")
        return False
    else:
        print("FAILURE: M2200 not confirmed active")
        print("  -> May be using Intel integrated GPU instead")
        return False

def main():
    print("="*80)
    print("M2200 MINIMAL TEST")
    print("="*80)
    print()
    print("This test uses tiny synthetic data (100x100 pixels)")
    print("to force GPU compute and verify M2200 CUDA cores fire.")
    print()
    
    # Create test data
    tiff_path = create_small_test_tiff()
    
    # Run test
    success = run_gpu_test(tiff_path)
    
    print()
    print("="*80)
    if success:
        print("M2200 CONFIRMED WORKING")
    else:
        print("M2200 NOT CONFIRMED - CHECK ABOVE")
    print("="*80)

if __name__ == "__main__":
    main()
