#!/usr/bin/env python3
"""
Direct CUDA test without nvrtc compilation
Uses pre-compiled kernels only
"""

from wreckhunter2000.scripts.tools.cuda_env import configure_cuda_environment
configure_cuda_environment()

import cupy as cp
import numpy as np

print("="*80)
print("CUDA M2200 TEST - NO COMPILATION")
print("="*80)

# Test 1: GPU detection
print("\n[1/3] GPU Detection...")
device = cp.cuda.Device(0)
props = cp.cuda.runtime.getDeviceProperties(0)
print(f"  GPU: {props['name'].decode()}")
print(f"  Compute: {props['major']}.{props['minor']}")
print(f"  Memory: {props['totalGlobalMem'] / 1024**3:.1f} GB")

# Test 2: Simple operations (no custom kernels)
print("\n[2/3] Basic GPU Operations...")
a = cp.random.rand(1000, 1000, dtype=cp.float32)
b = cp.random.rand(1000, 1000, dtype=cp.float32)
c = a + b
d = cp.mean(c)
print(f"  Matrix add: OK")
print(f"  Mean result: {float(d):.6f}")

# Test 3: Heavy computation
print("\n[3/3] Heavy GPU Workload...")
large = cp.random.rand(5000, 5000, dtype=cp.float32)
for i in range(10):
    result = cp.matmul(large, large)
    cp.cuda.Stream.null.synchronize()
print(f"  Matrix multiply (5000x5000) x10: OK")

print("\n" + "="*80)
print("[SUCCESS] M2200 CUDA CORES WORKING!")
print("="*80)
print("\nRun 'nvidia-smi' to see GPU utilization")
