#!/usr/bin/env python3
"""
Minimal CUDA test - CPU data to GPU only
"""

from wreckhunter2000.scripts.tools.cuda_env import configure_cuda_environment
configure_cuda_environment()

import cupy as cp
import numpy as np

print("="*80)
print("CUDA M2200 MINIMAL TEST")
print("="*80)

# GPU detection
print("\n[1/2] GPU Detection...")
device = cp.cuda.Device(0)
props = cp.cuda.runtime.getDeviceProperties(0)
print(f"  GPU: {props['name'].decode()}")
print(f"  Compute: {props['major']}.{props['minor']}")

# CPU to GPU transfer
print("\n[2/2] CPU -> GPU Transfer...")
cpu_data = np.array([1, 2, 3, 4, 5], dtype=np.float32)
print(f"  CPU data: {cpu_data}")

gpu_data = cp.asarray(cpu_data)
print(f"  Uploaded to GPU: OK")

result = cp.asnumpy(gpu_data)
print(f"  Downloaded from GPU: {result}")

if np.array_equal(cpu_data, result):
    print("\n" + "="*80)
    print("[SUCCESS] M2200 GPU MEMORY ACCESS WORKING!")
    print("="*80)
else:
    print("\n[FAIL] Data mismatch")
