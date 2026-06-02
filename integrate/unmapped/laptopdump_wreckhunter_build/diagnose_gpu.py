#!/usr/bin/env python3
"""
GPU Diagnostic - Verify Quadro M2200 CUDA cores are active
"""

import subprocess
import sys
from pathlib import Path

def check_nvidia_smi():
    """Check NVIDIA GPU via nvidia-smi"""
    print("=" * 80)
    print("NVIDIA GPU STATUS (nvidia-smi)")
    print("=" * 80)
    print()
    
    try:
        result = subprocess.run(["nvidia-smi"], capture_output=True, text=True)
        print(result.stdout)
        
        if "Quadro M2200" in result.stdout:
            print("✓ Quadro M2200 detected by nvidia-smi")
            return True
        elif "NVIDIA" in result.stdout:
            print("⚠ NVIDIA GPU detected but not Quadro M2200")
            return False
        else:
            print("✗ No NVIDIA GPU detected")
            return False
    except FileNotFoundError:
        print("✗ nvidia-smi not found (NVIDIA drivers not installed)")
        return False

def check_vulkan():
    """Check Vulkan runtime"""
    print()
    print("=" * 80)
    print("VULKAN RUNTIME STATUS")
    print("=" * 80)
    print()
    
    try:
        result = subprocess.run(["vulkaninfo", "--summary"], capture_output=True, text=True)
        
        if result.returncode == 0:
            print("✓ Vulkan runtime installed")
            
            if "NVIDIA" in result.stdout:
                print("✓ NVIDIA Vulkan driver detected")
                return True
            else:
                print("⚠ Vulkan installed but no NVIDIA driver")
                return False
        else:
            print("✗ Vulkan runtime not working")
            return False
    except FileNotFoundError:
        print("✗ vulkaninfo not found")
        print("Install Vulkan SDK: https://vulkan.lunarg.com/")
        return False

def check_rust_gpu():
    """Check Rust GPU engine"""
    print()
    print("=" * 80)
    print("RUST GPU ENGINE STATUS")
    print("=" * 80)
    print()
    
    exe = Path(__file__).parent / "target" / "release" / "cesarops-gpu.exe"
    
    if not exe.exists():
        print(f"✗ Rust GPU engine not built: {exe}")
        print("Run: build_gpu.bat")
        return False
    
    print(f"✓ Executable found: {exe}")
    print()
    print("Testing GPU initialization...")
    print()
    
    result = subprocess.run([str(exe)], capture_output=True, text=True, timeout=30)
    
    # Parse output for GPU info
    lines = result.stdout.split('\n')
    
    gpu_detected = False
    quadro_active = False
    
    for line in lines:
        print(line)
        
        if "Quadro M2200" in line:
            gpu_detected = True
            if "🟢" in line or "active" in line.lower():
                quadro_active = True
        elif "NVIDIA" in line and "vendor=0x10de" in line:
            gpu_detected = True
    
    print()
    
    if quadro_active:
        print("✓ Quadro M2200 is ACTIVE and will be used for processing")
        return True
    elif gpu_detected:
        print("⚠ NVIDIA GPU detected but Quadro M2200 not confirmed active")
        return False
    else:
        print("✗ No NVIDIA GPU detected by Rust engine")
        return False

def main():
    print("=" * 80)
    print("CESAROPS GPU DIAGNOSTIC")
    print("=" * 80)
    print()
    
    nvidia_ok = check_nvidia_smi()
    vulkan_ok = check_vulkan()
    rust_ok = check_rust_gpu()
    
    print()
    print("=" * 80)
    print("DIAGNOSTIC SUMMARY")
    print("=" * 80)
    print()
    print(f"NVIDIA Driver:    {'✓ PASS' if nvidia_ok else '✗ FAIL'}")
    print(f"Vulkan Runtime:   {'✓ PASS' if vulkan_ok else '✗ FAIL'}")
    print(f"Rust GPU Engine:  {'✓ PASS' if rust_ok else '✗ FAIL'}")
    print()
    
    if nvidia_ok and vulkan_ok and rust_ok:
        print("=" * 80)
        print("✓ ALL SYSTEMS GO - QUADRO M2200 READY FOR GPU PROCESSING")
        print("=" * 80)
        print()
        print("Next steps:")
        print("  1. python test_pipeline.py    # Test end-to-end")
        print("  2. python cesarops_cli.py     # Run full scan")
        return True
    else:
        print("=" * 80)
        print("✗ GPU NOT READY - FIX ISSUES ABOVE")
        print("=" * 80)
        print()
        
        if not nvidia_ok:
            print("Fix NVIDIA Driver:")
            print("  - Download from: https://www.nvidia.com/Download/index.aspx")
            print("  - Select: Quadro M2200")
            print()
        
        if not vulkan_ok:
            print("Fix Vulkan Runtime:")
            print("  - Download from: https://vulkan.lunarg.com/")
            print("  - Install Vulkan SDK")
            print()
        
        if not rust_ok:
            print("Fix Rust GPU Engine:")
            print("  - Run: build_gpu.bat")
            print("  - Check build errors")
            print()
        
        return False

if __name__ == "__main__":
    success = main()
    sys.exit(0 if success else 1)
