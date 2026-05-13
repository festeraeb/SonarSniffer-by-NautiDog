#!/bin/bash
# Quick P100 stress test using cuda_memcheck and nvidia-smi
echo "=== P100 Quick Stress Test ==="
echo "Running nvidia-smi -i 0 --gom=0 (compute mode)..."

# Use nvidia-smi to run a quick bandwidth test on each GPU
for GPU in 0 1; do
    echo ""
    echo "--- GPU $GPU ---"
    # Run a quick CUDA bandwidth test if available
    if command -v /usr/local/cuda/extras/demo_suite/bandwidthTest > /dev/null 2>&1; then
        /usr/local/cuda/extras/demo_suite/bandwidthTest --device=$GPU --mode=quick 2>&1 | grep -E "Host to Device|Device to Host|Device to Device"
    elif command -v cuda-memcheck > /dev/null 2>&1; then
        echo "cuda-memcheck available but no bandwidth test binary"
    else
        echo "No CUDA test binaries found — using python fallback"
    fi
done

# Python CUDA stress (if available)
python3 -c "
import time
try:
    import torch
    for gpu in range(2):
        torch.cuda.set_device(gpu)
        dev = torch.device(f'cuda:{gpu}')
        a = torch.randn(4096, 4096, device=dev)
        b = torch.randn(4096, 4096, device=dev)
        torch.cuda.synchronize()
        start = time.time()
        for _ in range(20):
            c = torch.matmul(a, b)
        torch.cuda.synchronize()
        elapsed = time.time() - start
        tflops = (2 * 4096**3 * 20) / elapsed / 1e12
        print(f'GPU {gpu}: 20x matmul(4096x4096) in {elapsed:.2f}s = {tflops:.2f} TFLOPS')
except ImportError:
    print('PyTorch not installed — skipping compute stress')
    print('Install with: pip3 install torch')
" 2>&1

echo ""
echo "=== Post-stress temps ==="
nvidia-smi --query-gpu=index,temperature.gpu,power.draw,memory.used --format=csv,noheader
