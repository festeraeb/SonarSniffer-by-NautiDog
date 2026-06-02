#!/usr/bin/env python3
"""
GPU Detection Test - Verify Quadro M2200 is visible
"""

import subprocess
import sys
from pathlib import Path

def test_rust_gpu():
    """Test Rust GPU engine initialization"""
    print("=" * 80)
    print("TESTING RUST GPU ENGINE")
    print("=" * 80)
    print()
    
    rust_exe = Path(__file__).parent / "target" / "release" / "cesarops-gpu.exe"
    
    if not rust_exe.exists():
        print("ERROR: Rust engine not built")
        print("Run: build_gpu.bat")
        return False
    
    print("Running GPU engine (no TIFF - just initialization test)...")
    print()
    
    result = subprocess.run(
        [str(rust_exe)],
        capture_output=True,
        text=True
    )
    
    print(result.stdout)
    
    if "Quadro M2200" in result.stdout:
        print("✓ SUCCESS: Quadro M2200 detected and active")
        return True
    elif "NVIDIA" in result.stdout:
        print("⚠ WARNING: NVIDIA GPU detected but not Quadro M2200")
        print("Check if correct GPU is selected")
        return False
    else:
        print("✗ FAILURE: No NVIDIA GPU detected")
        print("GPU may not be accessible or drivers missing")
        return False

def test_wgpu_info():
    """Test wgpu adapter enumeration"""
    print()
    print("=" * 80)
    print("TESTING WGPU ADAPTER ENUMERATION")
    print("=" * 80)
    print()
    
    try:
        import wgpu
        
        adapter = wgpu.gpu.request_adapter_sync(power_preference="high-performance")
        
        if adapter:
            info = adapter.info
            print(f"Adapter: {info['adapter_type']}")
            print(f"Backend: {info['backend_type']}")
            print(f"Device: {info['device']}")
            print(f"Vendor: {info['vendor']}")
            
            if "NVIDIA" in str(info):
                print("✓ NVIDIA GPU detected via wgpu-py")
                return True
        else:
            print("✗ No adapter found")
            return False
            
    except ImportError:
        print("wgpu-py not installed (optional)")
        print("Install with: pip install wgpu")
        return None
    except Exception as e:
        print(f"Error: {e}")
        return False

if __name__ == "__main__":
    rust_ok = test_rust_gpu()
    wgpu_ok = test_wgpu_info()
    
    print()
    print("=" * 80)
    print("SUMMARY")
    print("=" * 80)
    print(f"Rust GPU Engine: {'✓ PASS' if rust_ok else '✗ FAIL'}")
    print(f"wgpu-py Test: {'✓ PASS' if wgpu_ok else '⚠ SKIP' if wgpu_ok is None else '✗ FAIL'}")
    print()
    
    if rust_ok:
        print("GPU is ready. Run: python cesarops_cli.py")
    else:
        print("GPU not detected. Check:")
        print("  1. NVIDIA drivers installed")
        print("  2. Quadro M2200 enabled in Device Manager")
        print("  3. Vulkan runtime installed")
