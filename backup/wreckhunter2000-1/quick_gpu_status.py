#!/usr/bin/env python3
"""
QUICK GPU STATUS CHECK - Run this first!
Shows exactly what GPU state is on your Xeon right now

Usage:
    python3 quick_gpu_status.py
    
    # Or with custom Xeon IP:
    python3 quick_gpu_status.py 10.0.0.56
"""

import subprocess
import sys

def check_local():
    """Check GPUs on current machine"""
    print("🖥️  LOCAL MACHINE GPU STATUS")
    print("─" * 50)
    
    try:
        # nvidia-smi
        result = subprocess.run(['nvidia-smi', '-L'], capture_output=True, text=True, timeout=5)
        if result.returncode == 0:
            gpu_count = len(result.stdout.strip().split('\n'))
            print(f"✓ NVIDIA Driver found")
            print(f"✓ {gpu_count} GPU(s) detected:")
            for line in result.stdout.strip().split('\n'):
                if line.strip():
                    print(f"  • {line}")
        else:
            print("✗ nvidia-smi not available")
    except Exception as e:
        print(f"✗ Error: {e}")
    
    print()

def check_xeon(xeon_ip):
    """Check GPUs on remote Xeon"""
    print(f"🖥️  XEON MACHINE GPU STATUS ({xeon_ip})")
    print("─" * 50)
    
    # Test SSH connectivity first
    try:
        result = subprocess.run(
            f'ssh -o ConnectTimeout=5 -o StrictHostKeyChecking=no cesarops@{xeon_ip} "echo OK"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        if result.returncode != 0:
            print(f"✗ Cannot SSH to {xeon_ip}")
            print(f"  Try: ssh cesarops@{xeon_ip}")
            return False
    except:
        print(f"✗ SSH timeout to {xeon_ip}")
        return False
    
    print(f"✓ SSH connection OK")
    print()
    
    # nvidia-smi on Xeon
    print("Checking nvidia-smi...")
    try:
        result = subprocess.run(
            f'ssh -o ConnectTimeout=5 cesarops@{xeon_ip} "nvidia-smi -L"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        if result.returncode == 0:
            gpu_count = len([l for l in result.stdout.strip().split('\n') if l.strip()])
            print(f"  ✓ {gpu_count} GPU(s) detected via nvidia-smi:")
            for line in result.stdout.strip().split('\n'):
                if line.strip():
                    print(f"    • {line}")
            if gpu_count < 2:
                print(f"    ⚠ ISSUE: Only {gpu_count} GPU visible! (expected 2)")
        else:
            print(f"  ✗ nvidia-smi error on Xeon")
    except Exception as e:
        print(f"  ✗ Error: {e}")
    
    print()
    
    # CUDA enumeration
    print("Checking CUDA device count...")
    try:
        result = subprocess.run(
            f'ssh -o ConnectTimeout=5 cesarops@{xeon_ip} "python3 -c \\"import cupy as cp; print(f\\'Found {{cp.cuda.runtime.getDeviceCount()}} CUDA devices\\')\\"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        if result.returncode == 0:
            print(f"  ✓ {result.stdout.strip()}")
        else:
            print(f"  ✗ CuPy error (CUDA may not be initialized)")
    except Exception as e:
        print(f"  ✗ Error: {e}")
    
    print()
    
    # PCIe enumeration
    print("Checking PCIe GPU entries...")
    try:
        result = subprocess.run(
            f'ssh -o ConnectTimeout=5 cesarops@{xeon_ip} "lspci | grep -i nvidia | wc -l"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        count = int(result.stdout.strip())
        print(f"  ✓ {count} GPU(s) visible in PCIe:")
        result = subprocess.run(
            f'ssh -o ConnectTimeout=5 cesarops@{xeon_ip} "lspci | grep -i nvidia"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        for line in result.stdout.strip().split('\n'):
            if line.strip():
                print(f"    • {line[:70]}")
        if count < 2:
            print(f"    ⚠ ISSUE: Only {count} GPU in PCIe! (expected 2)")
    except Exception as e:
        print(f"  ✗ Error: {e}")
    
    print()
    return True

def main():
    print()
    print("╔" + "═"*50 + "╗")
    print("║  CESAROPS GPU STATUS CHECK                       ║")
    print("╚" + "═"*50 + "╝")
    print()
    
    # Check local
    check_local()
    
    # Check Xeon
    xeon_ip = sys.argv[1] if len(sys.argv) > 1 else "10.0.0.56"
    if check_xeon(xeon_ip):
        print("─" * 50)
        print()
        print("📋 WHAT THIS MEANS:")
        print()
        print("✓ If you see:")
        print("  • 'Found 2 CUDA devices'")
        print("  • '2 GPU(s) detected'")
        print("  → ✅ GPUs are properly recognized!")
        print()
        print("✗ If you see:")
        print("  • 'Found 1 CUDA devices' (should be 2)")
        print("  • 'Only 1 GPU visible' in nvidia-smi")
        print("  → ⚠️  BIOS issue - follow CESAROPS3_GPU_FIX.md Step 3")
        print()
        print("Next:")
        print("  1. If both GPUs show → Run: python3 scripts/deploy_cesarops3.py --action launch")
        print("  2. If only 1 shows   → Access BIOS on Xeon and enable second PCIe slot")
        print()

if __name__ == "__main__":
    main()
