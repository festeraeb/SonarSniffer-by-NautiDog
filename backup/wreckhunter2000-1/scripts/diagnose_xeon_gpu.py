#!/usr/bin/env python3
"""
DIAGNOSTIC SCRIPT: Xeon GPU Visibility & Driver Check
Runs via SSH to diagnose BIOS 4g bar issue and GPU enumeration

Usage:
    python3 diagnose_xeon_gpu.py <host> <user>
    # or defaults to 10.0.0.56 / cesarops
"""

import subprocess
import sys
import json

XENON_HOST = "10.0.0.56"
XENON_USER = "cesarops"

def ssh_run(host, user, cmd):
    """Execute command via SSH and return (success, output)"""
    try:
        result = subprocess.run(
            f'ssh -o ConnectTimeout=10 {user}@{host} "{cmd}"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=30
        )
        return result.returncode == 0, result.stdout.strip(), result.stderr.strip()
    except subprocess.TimeoutExpired:
        return False, "", "Timeout (30s)"
    except Exception as e:
        return False, "", str(e)

def diagnose():
    print("╔" + "═"*78 + "╗")
    print("║ CESAROPS3 (XEON) GPU DIAGNOSTIC REPORT                                      ║")
    print("║ Post-BIOS Update: 4G BAR Issue Remediation                                  ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    if len(sys.argv) > 1:
        XENON_HOST = sys.argv[1]
    if len(sys.argv) > 2:
        XENON_USER = sys.argv[2]
    
    print(f"Target: {XENON_USER}@{XENON_HOST}")
    print("─" * 80)
    print()
    
    # TEST 1: nvidia-smi
    print("┌─ TEST 1: NVIDIA Driver Status (nvidia-smi)")
    print("│")
    success, out, err = ssh_run(XENON_HOST, XENON_USER, "nvidia-smi --query-gpu=index,name,driver_version,memory.total --format=csv,noheader")
    if success:
        lines = out.strip().split('\n')
        gpu_count = len(lines)
        print(f"│  ✓ Driver installed")
        print(f"│  └─ GPUs detected: {gpu_count}")
        for i, line in enumerate(lines, 1):
            parts = [p.strip() for p in line.split(',')]
            print(f"│     GPU {i}: {parts[1]} | Driver: {parts[2]} | Memory: {parts[3]}")
    else:
        print(f"│  ✗ Error: {err or 'Driver not loaded'}")
    print()
    
    # TEST 2: CUDA enumeration
    print("┌─ TEST 2: CUDA Device Enumeration (cupy)")
    print("│")
    success, out, err = ssh_run(XENON_HOST, XENON_USER, 
        "python3 -c \"import cupy as cp; print(f'CUDA Devices: {cp.cuda.runtime.getDeviceCount()}')\"")
    if success:
        print(f"│  ✓ {out}")
    else:
        print(f"│  ✗ CuPy error: {err or 'Not installed or CUDA error'}")
    print()
    
    # TEST 3: Compute capabilities
    print("┌─ TEST 3: GPU Compute Capabilities & Memory")
    print("│")
    cuda_info_script = """
import cupy as cp
count = cp.cuda.runtime.getDeviceCount()
if count == 0:
    print('ERROR: No CUDA devices found')
else:
    for i in range(count):
        dev = cp.cuda.Device(i)
        props = cp.cuda.runtime.getDeviceProperties(i)
        name = props.get('name', b'Unknown').decode() if isinstance(props.get('name'), bytes) else props.get('name', 'Unknown')
        mem_mb = props.get('totalGlobalMem', 0) // (1024*1024)
        compute = f"{props.get('major', 0)}.{props.get('minor', 0)}"
        print(f'GPU {i}: {name} | Compute: {compute} | Memory: {mem_mb} MB')
"""
    success, out, err = ssh_run(XENON_HOST, XENON_USER, f"python3 -c '{cuda_info_script}'")
    if success:
        for line in out.strip().split('\n'):
            print(f"│  {line}")
    else:
        print(f"│  ✗ Error: {err}")
    print()
    
    # TEST 4: PCIe Bus Configuration
    print("┌─ TEST 4: PCIe Bus Configuration (lspci)")
    print("│")
    success, out, err = ssh_run(XENON_HOST, XENON_USER, "lspci | grep -i nvidia")
    if success:
        count = len(out.strip().split('\n'))
        print(f"│  PCIe GPU Entries: {count}")
        for line in out.strip().split('\n'):
            print(f"│  └─ {line[:70]}")
    else:
        print(f"│  ✗ Error: {err or 'lspci not available'}")
    print()
    
    # TEST 5: BIOS Power Settings
    print("┌─ TEST 5: Power Management (GPU clocks)")
    print("│")
    success, out, err = ssh_run(XENON_HOST, XENON_USER, "nvidia-smi -q | grep -A5 'Clock Throttle Reasons'")
    if success:
        print(f"│  {out.replace(chr(10), chr(10) + '│  ')}")
    else:
        print(f"│  (Skipped - requires nvidia-smi -q)")
    print()
    
    # TEST 6: Kernel logs
    print("┌─ TEST 6: Recent Kernel Logs (GPU-related)")
    print("│")
    success, out, err = ssh_run(XENON_HOST, XENON_USER, "sudo dmesg | grep -i nvidia | tail -5")
    if success and out:
        for line in out.strip().split('\n'):
            print(f"│  └─ {line[:70]}")
    else:
        print(f"│  (Kernel logs not accessible or empty)")
    print()
    
    # SUMMARY
    print("╔" + "═"*78 + "╗")
    print("║ DIAGNOSTIC SUMMARY                                                          ║")
    print("├" + "─"*78 + "┤")
    print("║                                                                              ║")
    print("║  NEXT STEPS if only 1 GPU showing:                                          ║")
    print("║  1. Check BIOS settings: PCIe Slot Configuration / IOMMU / 4G Decoding       ║")
    print("║  2. Verify second slot has power connection (GTX 1060)                       ║")
    print("║  3. Reseat second GPU or try different PCIe slot                             ║")
    print("║  4. Update NVIDIA drivers: apt-get install -y nvidia-driver-XXX             ║")
    print("║  5. Reboot and rerun this diagnostic                                         ║")
    print("║                                                                              ║")
    print("╚" + "═"*78 + "╝")

if __name__ == "__main__":
    diagnose()
