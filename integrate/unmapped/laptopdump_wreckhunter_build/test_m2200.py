#!/usr/bin/env python3
"""
Test M2200 GPU with synthetic thermal data
Creates a test TIFF and processes it through the GPU engine
"""

import numpy as np
import subprocess
from pathlib import Path

def create_test_tiff():
    """Create synthetic thermal TIFF with known anomalies"""
    try:
        from PIL import Image
    except ImportError:
        print("Installing PIL...")
        subprocess.run(["pip", "install", "pillow"], check=True)
        from PIL import Image
    
    # Create 1000x1000 thermal image
    width, height = 1000, 1000
    
    # Base temperature: 285K (typical water surface)
    thermal_data = np.random.normal(285.0, 5.0, (height, width)).astype(np.float32)
    
    # Add 10 cold anomalies (thermal sinks - aircraft aluminum)
    print("Adding synthetic anomalies...")
    anomaly_coords = []
    for i in range(10):
        x = np.random.randint(100, width - 100)
        y = np.random.randint(100, height - 100)
        
        # Create cold spot (10K below ambient)
        for dy in range(-5, 6):
            for dx in range(-5, 6):
                if 0 <= y+dy < height and 0 <= x+dx < width:
                    thermal_data[y+dy, x+dx] -= 10.0
        
        anomaly_coords.append((x, y))
        print(f"  Anomaly {i+1}: ({x}, {y})")
    
    # Convert to uint16 for TIFF (scale to 0-65535)
    thermal_u16 = ((thermal_data - 250) * 200).clip(0, 65535).astype(np.uint16)
    
    # Save as TIFF
    output_path = Path("test_thermal.tif")
    img = Image.fromarray(thermal_u16, mode='I;16')
    img.save(output_path)
    
    print(f"\nCreated test TIFF: {output_path}")
    print(f"  Size: {width}x{height}")
    print(f"  Anomalies: {len(anomaly_coords)}")
    
    return output_path, anomaly_coords

def run_gpu_test(tiff_path):
    """Run GPU engine on test TIFF"""
    exe = Path("target/release/cesarops-gpu.exe")
    
    if not exe.exists():
        print(f"ERROR: {exe} not found")
        print("Run: cargo build --release --bin cesarops-gpu")
        return False
    
    print("\n" + "=" * 80)
    print("RUNNING GPU TEST")
    print("=" * 80)
    print()
    
    result = subprocess.run(
        [str(exe), str(tiff_path), "--threshold", "2.0"],
        capture_output=True,
        text=True
    )
    
    print(result.stdout)
    
    if result.returncode != 0:
        print("ERROR:", result.stderr)
        return False
    
    # Check for M2200 detection
    if "Quadro M2200" in result.stdout:
        print("\nSUCCESS: Quadro M2200 detected and used")
        
        # Check for anomaly detection
        if "Detected" in result.stdout and "anomalies" in result.stdout:
            print("SUCCESS: Anomalies detected by GPU")
            return True
    
    return False

def main():
    print("=" * 80)
    print("M2200 GPU TEST WITH SYNTHETIC DATA")
    print("=" * 80)
    print()
    
    # Create test TIFF
    tiff_path, expected_anomalies = create_test_tiff()
    
    # Run GPU test
    success = run_gpu_test(tiff_path)
    
    print("\n" + "=" * 80)
    if success:
        print("M2200 GPU TEST PASSED")
        print("=" * 80)
        print("\nThe Quadro M2200 is working and processing TIFFs.")
        print("Next: Run with real satellite data")
    else:
        print("M2200 GPU TEST FAILED")
        print("=" * 80)
        print("\nCheck errors above")

if __name__ == "__main__":
    main()
