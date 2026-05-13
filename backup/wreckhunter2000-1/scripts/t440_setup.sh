#!/bin/bash
# T440 Setup: GPU stress test + Cloudflare tunnel + Coral TPU
set -e

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  T440 Setup: P100 Stress + Cloudflare + Coral TPU           ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# ── 1. Quick GPU Stress Test (both P100s) ─────────────────────────────────────
echo ""
echo "=== [1/3] P100 Stress Test (30 seconds each) ==="
echo ""

# Use nvidia-smi to run a quick compute stress via CUDA
# gpu-burn is ideal but we'll use a simple matrix multiply via Python
python3 -c "
import subprocess, time, threading

def stress_gpu(gpu_id):
    '''Run a quick matrix multiply stress on one GPU'''
    code = f'''
import torch
import time
torch.cuda.set_device({gpu_id})
device = torch.device(f\"cuda:{gpu_id}\")
print(f\"GPU {gpu_id}: Starting stress test...\")
start = time.time()
# Allocate large matrices and multiply repeatedly
a = torch.randn(4096, 4096, device=device)
b = torch.randn(4096, 4096, device=device)
for i in range(50):
    c = torch.matmul(a, b)
    torch.cuda.synchronize()
elapsed = time.time() - start
print(f\"GPU {gpu_id}: 50 matmuls (4096x4096) in {elapsed:.2f}s = {50/elapsed:.1f} ops/s\")
# Check memory
mem = torch.cuda.memory_allocated(device) / 1024**2
print(f\"GPU {gpu_id}: Memory used: {mem:.0f} MiB\")
'''
    result = subprocess.run(['python3', '-c', code], capture_output=True, text=True, timeout=60)
    print(result.stdout.strip())
    if result.stderr:
        # Filter out just errors, not warnings
        errors = [l for l in result.stderr.split('\n') if 'error' in l.lower() or 'Error' in l]
        if errors:
            print(f'GPU {gpu_id} ERRORS: {errors}')

# Run both GPUs in parallel
t0 = threading.Thread(target=stress_gpu, args=(0,))
t1 = threading.Thread(target=stress_gpu, args=(1,))
t0.start()
t1.start()
t0.join()
t1.join()
print('Stress test complete.')
" 2>&1 || echo "PyTorch not available — using nvidia-smi dmon instead"

# Fallback: just check both GPUs respond
echo ""
nvidia-smi --query-gpu=index,name,temperature.gpu,power.draw,utilization.gpu,memory.used --format=csv,noheader
echo ""

# ── 2. Cloudflare Tunnel ──────────────────────────────────────────────────────
echo "=== [2/3] Cloudflare Tunnel Setup ==="
if [ -f ~/wreckhunter2000-1/scripts/setup_cesarops_tunnel.sh ]; then
    bash ~/wreckhunter2000-1/scripts/setup_cesarops_tunnel.sh
else
    echo "Tunnel script not found at ~/wreckhunter2000-1/scripts/setup_cesarops_tunnel.sh"
    echo "Checking if cloudflared is already running..."
    systemctl status cloudflared --no-pager 2>/dev/null | head -5 || echo "cloudflared not installed"
fi

# ── 3. Coral TPU Setup ────────────────────────────────────────────────────────
echo ""
echo "=== [3/3] Coral TPU Setup ==="
echo "PCIe device:"
lspci | grep -i coral
echo ""

# Check if gasket/apex driver is loaded
if lsmod | grep -q gasket; then
    echo "gasket module already loaded"
else
    echo "Loading gasket/apex drivers..."
    # Install the Coral PCIe driver (gasket-dkms)
    if ! dpkg -l | grep -q gasket-dkms; then
        echo "Installing gasket-dkms..."
        echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" | sudo tee /etc/apt/sources.list.d/coral-edgetpu.list
        curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | sudo apt-key add -
        sudo apt-get update -qq
        sudo apt-get install -y gasket-dkms libedgetpu1-std
    fi
    sudo modprobe gasket
    sudo modprobe apex
fi

# Check if device appeared
if ls /dev/apex_0 2>/dev/null; then
    echo "✓ Coral TPU online at /dev/apex_0"
else
    echo "✗ /dev/apex_0 not found"
    echo "Checking dmesg for apex/gasket..."
    dmesg | grep -i "apex\|gasket\|coral" | tail -5
fi

echo ""
echo "=== Setup Complete ==="
nvidia-smi --query-gpu=index,name,temperature.gpu,memory.free --format=csv,noheader
ls /dev/apex* 2>/dev/null && echo "TPU: ONLINE" || echo "TPU: OFFLINE"
systemctl is-active cloudflared 2>/dev/null && echo "Tunnel: ACTIVE" || echo "Tunnel: INACTIVE"
