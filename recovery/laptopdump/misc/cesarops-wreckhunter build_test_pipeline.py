#!/usr/bin/env python3
"""
End-to-End Pipeline Test
Tests: Build → GPU Detection → Single TIFF Processing
"""

import subprocess
import sys
from pathlib import Path

def step(num, total, msg):
    print(f"\n[{num}/{total}] {msg}")
    print("-" * 80)

def run_command(cmd, cwd=None):
    """Run command and return success status"""
    result = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True)
    print(result.stdout)
    if result.stderr:
        print(result.stderr)
    return result.returncode == 0

def main():
    print("=" * 80)
    print("CESAROPS END-TO-END PIPELINE TEST")
    print("=" * 80)
    
    root = Path(__file__).parent
    
    # Step 1: Build
    step(1, 4, "Building Rust GPU Engine")
    if not run_command("cargo build --release --bin cesarops-gpu", cwd=root):
        print("✗ Build failed")
        return False
    
    exe = root / "target" / "release" / "cesarops-gpu.exe"
    if not exe.exists():
        print(f"✗ Executable not found: {exe}")
        return False
    print(f"✓ Built: {exe}")
    
    # Step 2: GPU Detection
    step(2, 4, "Testing GPU Detection")
    result = subprocess.run([str(exe)], capture_output=True, text=True)
    print(result.stdout)
    
    if "Quadro M2200" in result.stdout:
        print("✓ Quadro M2200 detected")
    elif "NVIDIA" in result.stdout:
        print("⚠ NVIDIA GPU detected (not Quadro M2200)")
    else:
        print("✗ No NVIDIA GPU detected")
        return False
    
    # Step 3: Find Test TIFF
    step(3, 4, "Finding Test TIFF")
    data_dir = Path(r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw")
    
    test_tiff = None
    for pattern in ["**/*B10.tif", "**/*B11.tif"]:
        tiffs = list(data_dir.glob(pattern))
        if tiffs:
            test_tiff = tiffs[0]
            break
    
    if not test_tiff:
        print(f"✗ No thermal TIFFs found in {data_dir}")
        print("Run fetcher.py first to download satellite data")
        return False
    
    print(f"✓ Found test TIFF: {test_tiff.name}")
    
    # Step 4: Process TIFF
    step(4, 4, "Processing TIFF with GPU")
    result = subprocess.run([str(exe), str(test_tiff)], capture_output=True, text=True)
    print(result.stdout)
    
    if "GPU processing complete" in result.stdout:
        print("✓ GPU processing successful")
    else:
        print("✗ GPU processing failed")
        return False
    
    # Summary
    print("\n" + "=" * 80)
    print("✓ ALL TESTS PASSED")
    print("=" * 80)
    print("\nPipeline is ready. Run:")
    print("  python cesarops_cli.py")
    return True

if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
