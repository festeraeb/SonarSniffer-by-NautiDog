#!/usr/bin/env bash
# =============================================================================
# enable_gpu_p1000.sh  —  Enable NVIDIA Quadro P1000 on H97/Xeon node
#
# Run AFTER the P1000 is physically installed and the system has booted.
# Prerequisites: setup_h97_xeon.sh already run.
#
#   bash scripts/enable_gpu_p1000.sh
# =============================================================================

set -euo pipefail

REPO_DIR="$HOME/cesarops-core/wreckhunter2000"
VENV_DIR="$HOME/.venv/cesarops"
CUDA_VERSION="12.4"

echo "============================================================"
echo " CESARops P1000 GPU Enable Script"
echo " NVIDIA Quadro P1000 / Pascal / CUDA ${CUDA_VERSION}"
echo "============================================================"
echo ""

# ── [1/6] Confirm GPU is present ────────────────────────────────────────────
echo "[1/6] Checking for NVIDIA GPU..."
if ! lspci | grep -qi nvidia; then
    echo "  ✗ No NVIDIA GPU detected in lspci output."
    echo "    Make sure the P1000 is seated in the PCIe slot and power cycled."
    exit 1
fi
lspci | grep -i nvidia
echo "  ✓ NVIDIA GPU detected"

# ── [2/6] Install NVIDIA driver ──────────────────────────────────────────────
echo "[2/6] Installing NVIDIA driver (ubuntu-drivers)..."
sudo apt-get update -qq
sudo apt-get install -y ubuntu-drivers-common

# Recommended driver for Quadro P1000 on Ubuntu 22.04 is 535+
RECOMMENDED=$(ubuntu-drivers devices 2>/dev/null | grep "recommended" | awk '{print $3}' | head -1 || echo "nvidia-driver-535")
echo "  Recommended driver: $RECOMMENDED"
sudo apt-get install -y "$RECOMMENDED" nvidia-utils-535 2>/dev/null || \
    sudo apt-get install -y nvidia-driver-535 nvidia-utils-535
echo "  ✓ NVIDIA driver installed"

# ── [3/6] Install CUDA toolkit ───────────────────────────────────────────────
echo "[3/6] Installing CUDA ${CUDA_VERSION} toolkit..."
if ! command -v nvcc &>/dev/null; then
    wget -q "https://developer.download.nvidia.com/compute/cuda/repos/ubuntu2204/x86_64/cuda-keyring_1.1-1_all.deb"
    sudo dpkg -i cuda-keyring_1.1-1_all.deb
    rm -f cuda-keyring_1.1-1_all.deb
    sudo apt-get update -qq
    sudo apt-get install -y cuda-toolkit-12-4
    echo 'export PATH=/usr/local/cuda/bin:$PATH' >> ~/.bashrc
    echo 'export LD_LIBRARY_PATH=/usr/local/cuda/lib64:$LD_LIBRARY_PATH' >> ~/.bashrc
    export PATH=/usr/local/cuda/bin:$PATH
    export LD_LIBRARY_PATH=/usr/local/cuda/lib64:$LD_LIBRARY_PATH
fi
echo "  ✓ CUDA toolkit ready"

# ── [4/6] Install GPU Python packages ───────────────────────────────────────
echo "[4/6] Installing GPU Python packages (cupy, torch)..."
"$VENV_DIR/bin/pip" install --upgrade pip -q

# CuPy for CUDA 12.x
"$VENV_DIR/bin/pip" install cupy-cuda12x -q

# PyTorch with CUDA 12.4
"$VENV_DIR/bin/pip" install torch torchvision \
    --index-url https://download.pytorch.org/whl/cu124 -q

# GPU extras from repo if present
if [[ -f "$REPO_DIR/requirements-gpu.txt" ]]; then
    "$VENV_DIR/bin/pip" install -r "$REPO_DIR/requirements-gpu.txt" -q
fi
echo "  ✓ GPU Python packages installed"

# ── [5/6] Update .env for GPU node ──────────────────────────────────────────
echo "[5/6] Updating .env with GPU capabilities..."
ENV_FILE="$REPO_DIR/.env"
sed -i \
    -e 's/^NODE_HAS_GPU=false/NODE_HAS_GPU=true/' \
    -e 's/^NODE_GPU=IntelHD_integrated/NODE_GPU=QuadroP1000/' \
    -e 's/^NODE_HAS_CUDA=false/NODE_HAS_CUDA=true/' \
    -e 's/^NODE_VRAM_GB=0/NODE_VRAM_GB=4/' \
    -e 's/^NODE_TRAIN_IDLE=false/NODE_TRAIN_IDLE=true/' \
    "$ENV_FILE"
echo "  ✓ .env updated — NODE_HAS_GPU=true, NODE_GPU=QuadroP1000, VRAM=4GB"

# ── [6/6] Verify  ────────────────────────────────────────────────────────────
echo "[6/6] Verification (requires reboot first if driver just installed)..."
echo "  NOTE: If nvidia-smi fails here, reboot and re-run this step manually:"
echo "        nvidia-smi && python3 -c 'import cupy; print(cupy.cuda.Device().name)'"
echo ""

if command -v nvidia-smi &>/dev/null && nvidia-smi &>/dev/null; then
    nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader
    "$VENV_DIR/bin/python" -c "
import cupy
d = cupy.cuda.Device(0)
props = cupy.cuda.runtime.getDeviceProperties(0)
name = props['name']
if isinstance(name, bytes): name = name.decode()
vram = props['totalGlobalMem'] // 1024**2
print(f'  GPU: {name}  VRAM: {vram} MB')
print('  CuPy OK')
"
else
    echo "  ⚠ nvidia-smi not yet active — reboot required to load driver."
fi

echo ""
echo "============================================================"
echo " P1000 GPU SETUP COMPLETE"
echo "============================================================"
echo ""
echo " Reboot to activate the NVIDIA driver if not already done:"
echo "   sudo reboot"
echo ""
echo " After reboot, restart the worker:"
echo "   sudo systemctl restart cesarops-worker"
echo ""
echo " The worker will now use GPU acceleration for scan jobs."
echo "============================================================"
