#!/usr/bin/env python3
"""
CUDA Toolkit Verification
Run this after installing CUDA Toolkit to verify M2200 is accessible
"""

import sys

def test_cuda_toolkit():
    """Test if CUDA Toolkit is properly installed"""
    print("="*80)
    print("CUDA TOOLKIT VERIFICATION")
    print("="*80)
    print()
    
    # Test 1: CuPy import
    print("[1/4] Testing CuPy import...")
    try:
        import cupy as cp
        print("  [OK] CuPy imported successfully")
    except ImportError as e:
        print(f"  [FAIL] CuPy not installed: {e}")
        print("  Run: pip install cupy-cuda13x")
        return False
    except Exception as e:
        print(f"  [FAIL] CuPy import failed: {e}")
        print("  CUDA Toolkit may not be installed correctly")
        return False
    
    # Test 2: GPU detection
    print("\n[2/4] Testing GPU detection...")
    try:
        device = cp.cuda.Device(0)
        props = cp.cuda.runtime.getDeviceProperties(0)
        gpu_name = props['name'].decode()
        compute_cap = device.compute_capability
        
        print(f"  [OK] GPU Detected: {gpu_name}")
        print(f"  [OK] Compute Capability: {compute_cap}")
        print(f"  [OK] Total Memory: {props['totalGlobalMem'] / 1024**3:.1f} GB")
        
        if "M2200" not in gpu_name:
            print(f"  [WARN] Expected Quadro M2200, got {gpu_name}")
    except Exception as e:
        print(f"  [FAIL] GPU detection failed: {e}")
        return False
    
    # Test 3: Simple CUDA operation
    print("\n[3/4] Testing CUDA operations...")
    try:
        # Create array on GPU
        a = cp.array([1, 2, 3, 4, 5], dtype=cp.float32)
        b = cp.array([5, 4, 3, 2, 1], dtype=cp.float32)
        
        # Compute on GPU
        c = a + b
        result = cp.asnumpy(c)
        
        expected = [6, 6, 6, 6, 6]
        if list(result) == expected:
            print("  [OK] CUDA operations working")
        else:
            print(f"  [FAIL] Unexpected result: {result}")
            return False
    except Exception as e:
        print(f"  [FAIL] CUDA operations failed: {e}")
        return False
    
    # Test 4: Check nvidia-smi during GPU usage
    print("\n[4/4] Testing GPU utilization...")
    try:
        import subprocess
        import time
        
        # Do heavy GPU work
        print("  Running GPU computation...")
        large_array = cp.random.random((5000, 5000), dtype=cp.float32)
        for _ in range(10):
            result = cp.sum(large_array * large_array)
            cp.cuda.Stream.null.synchronize()
        
        # Check nvidia-smi
        result = subprocess.run(["nvidia-smi", "--query-gpu=utilization.gpu", "--format=csv,noheader,nounits"],
                              capture_output=True, text=True)
        gpu_util = result.stdout.strip()
        
        print(f"  GPU Utilization: {gpu_util}%")
        
        if int(gpu_util) > 0:
            print("  [OK] M2200 CUDA cores are being used!")
        else:
            print("  [WARN] GPU utilization is 0% - may need to run heavier workload")
    except Exception as e:
        print(f"  [WARN] Could not check GPU utilization: {e}")
    
    print()
    print("="*80)
    print("[SUCCESS] CUDA TOOLKIT VERIFIED - M2200 READY")
    print("="*80)
    print()
    print("Next steps:")
    print("  1. Run: python cuda_direct.py")
    print("  2. Watch nvidia-smi for GPU utilization")
    print("  3. Process your satellite TIFFs on M2200 CUDA cores")
    
    return True

if __name__ == "__main__":
    success = test_cuda_toolkit()
    sys.exit(0 if success else 1)
