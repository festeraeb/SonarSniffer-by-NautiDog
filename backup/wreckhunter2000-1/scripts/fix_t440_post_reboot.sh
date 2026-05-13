#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# T440 Post-Reboot Fix Script
# ═══════════════════════════════════════════════════════════════════════════════
# Run ON the T440 as cesarops (will prompt for sudo).
# Fixes: external drive mount, moves LLMs/cache to external, installs
# nvidia-driver-580 (Pascal legacy), fixes Coral TPU gasket driver.
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

EXTERNAL="/mnt/data-external"
MODELS_DIR="$EXTERNAL/cesarops/models"
CACHE_DIR="$EXTERNAL/cesarops/cache"
APT_CACHE="$EXTERNAL/cesarops/apt-cache"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[+]${NC} $*"; }
warn()  { echo -e "${YELLOW}[!]${NC} $*"; }
error() { echo -e "${RED}[X]${NC} $*"; }

# Sudo helper
SUDO_PASS="${SUDO_PASS:-cesarops}"
run_sudo() {
    echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null
}

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  T440 Post-Reboot Fix                                       ║"
echo "║  Driver 580 + External Drive + TPU                          ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# STEP 1: Mount external drive + fstab
# ══════════════════════════════════════════════════════════════════════════════
info "Step 1: External drive setup..."

run_sudo mkdir -p "$EXTERNAL"

if ! mountpoint -q "$EXTERNAL"; then
    run_sudo mount /dev/sdb2 "$EXTERNAL"
    info "Mounted /dev/sdb2 → $EXTERNAL"
else
    info "Already mounted"
fi

# Add to fstab if not there
if ! grep -q "dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9" /etc/fstab; then
    echo "UUID=dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9 $EXTERNAL ext4 defaults,nofail 0 2" | run_sudo tee -a /etc/fstab > /dev/null
    info "Added to /etc/fstab"
else
    info "Already in fstab"
fi

df -h "$EXTERNAL" | tail -1
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# STEP 2: Move heavy stuff to external drive
# ══════════════════════════════════════════════════════════════════════════════
info "Step 2: Moving LLMs and caches to external drive..."

# Create directory structure
mkdir -p "$MODELS_DIR"
mkdir -p "$CACHE_DIR/huggingface"
mkdir -p "$CACHE_DIR/ollama"
mkdir -p "$APT_CACHE"

# Move apt cache to external (frees space for driver install)
info "Redirecting apt cache to external..."
run_sudo rm -rf /var/cache/apt/archives
run_sudo ln -sf "$APT_CACHE" /var/cache/apt/archives
run_sudo mkdir -p "$APT_CACHE/partial"
run_sudo chown -R root:root "$APT_CACHE"

# Move HuggingFace cache (Cake models go here)
if [ -d "$HOME/.cache/huggingface" ] && [ ! -L "$HOME/.cache/huggingface" ]; then
    if [ "$(du -sm $HOME/.cache/huggingface 2>/dev/null | cut -f1)" -gt 100 ]; then
        info "Moving ~/.cache/huggingface → external..."
        rsync -a "$HOME/.cache/huggingface/" "$CACHE_DIR/huggingface/"
        rm -rf "$HOME/.cache/huggingface"
    else
        rm -rf "$HOME/.cache/huggingface"
    fi
    ln -sf "$CACHE_DIR/huggingface" "$HOME/.cache/huggingface"
    info "Symlinked ~/.cache/huggingface → $CACHE_DIR/huggingface"
elif [ -L "$HOME/.cache/huggingface" ]; then
    info "HuggingFace cache already symlinked"
else
    mkdir -p "$HOME/.cache"
    ln -sf "$CACHE_DIR/huggingface" "$HOME/.cache/huggingface"
    info "Created symlink ~/.cache/huggingface → $CACHE_DIR/huggingface"
fi

# Move Ollama models to external
OLLAMA_MODELS="/usr/share/ollama/.ollama/models"
if [ -d "$OLLAMA_MODELS" ] && [ ! -L "$OLLAMA_MODELS" ]; then
    info "Moving Ollama models → external..."
    run_sudo rsync -a "$OLLAMA_MODELS/" "$CACHE_DIR/ollama/"
    run_sudo rm -rf "$OLLAMA_MODELS"
    run_sudo ln -sf "$CACHE_DIR/ollama" "$OLLAMA_MODELS"
elif [ -d "/root/.ollama/models" ] && [ ! -L "/root/.ollama/models" ]; then
    run_sudo rsync -a "/root/.ollama/models/" "$CACHE_DIR/ollama/"
    run_sudo rm -rf "/root/.ollama/models"
    run_sudo ln -sf "$CACHE_DIR/ollama" "/root/.ollama/models"
fi

# Ensure models dir has the GGUF
if [ ! -f "$MODELS_DIR/Qwen3.6-35B-A3B-MXFP4_MOE.gguf" ]; then
    warn "Model GGUF not found in $MODELS_DIR — check if it needs copying"
    ls "$MODELS_DIR"/*.gguf 2>/dev/null || echo "  (no GGUFs found)"
fi

# Move ~/.local (pip packages, etc) if huge
LOCAL_SIZE=$(du -sm "$HOME/.local" 2>/dev/null | cut -f1)
if [ "${LOCAL_SIZE:-0}" -gt 2000 ] && [ ! -L "$HOME/.local" ]; then
    info "Moving ~/.local ($LOCAL_SIZE MB) → external..."
    mkdir -p "$EXTERNAL/cesarops/local"
    rsync -a "$HOME/.local/" "$EXTERNAL/cesarops/local/"
    rm -rf "$HOME/.local"
    ln -sf "$EXTERNAL/cesarops/local" "$HOME/.local"
fi

echo ""
info "Disk after cleanup:"
df -h / | tail -1
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# STEP 3: Install NVIDIA 580 driver (Pascal legacy)
# ══════════════════════════════════════════════════════════════════════════════
info "Step 3: Installing nvidia-driver-580 (Pascal P100 legacy branch)..."

# Remove the broken 595.71 and install 580
run_sudo apt-get update -qq 2>&1 | tail -3

# Remove gasket-dkms first (it's broken and blocks dpkg)
run_sudo dpkg --configure -a 2>&1 | tail -5 || true
run_sudo apt-get remove -y gasket-dkms 2>&1 | tail -3 || true

# Now install 580
info "Installing nvidia-driver-580 (this takes a few minutes for DKMS)..."
run_sudo apt-get install -y --allow-downgrades nvidia-driver-580 2>&1 | tail -10

# Verify
if modinfo nvidia 2>/dev/null | grep -q "580"; then
    info "nvidia-driver-580 kernel module installed"
else
    warn "Module install may need a reboot to take effect"
fi

echo ""

# ══════════════════════════════════════════════════════════════════════════════
# STEP 4: Fix Coral Edge TPU (gasket driver)
# ══════════════════════════════════════════════════════════════════════════════
info "Step 4: Fixing Coral Edge TPU..."

# Check PCIe slot
TPU_PCI=$(lspci | grep -i "coral\|apex\|google" | head -1)
if [ -z "$TPU_PCI" ]; then
    error "No Coral TPU detected in PCIe slots"
    echo "  Check physical seating in slot 6"
else
    info "TPU found: $TPU_PCI"
fi

# Install gasket + apex drivers for current kernel
KERNEL=$(uname -r)
info "Kernel: $KERNEL"

# Add Coral package repo if not present
if [ ! -f /etc/apt/sources.list.d/coral-edgetpu.list ]; then
    info "Adding Coral Edge TPU repository..."
    echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" | run_sudo tee /etc/apt/sources.list.d/coral-edgetpu.list > /dev/null
    curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | run_sudo apt-key add - 2>/dev/null
    run_sudo apt-get update -qq 2>&1 | tail -3
fi

# Install gasket-dkms (builds for current kernel)
info "Installing gasket-dkms for kernel $KERNEL..."
run_sudo apt-get install -y gasket-dkms libedgetpu1-std 2>&1 | tail -10 || {
    warn "gasket-dkms failed from repo. Trying from source..."
    # Fallback: build gasket from Google's git
    if [ ! -d /tmp/gasket-driver ]; then
        git clone https://github.com/google/gasket-driver.git /tmp/gasket-driver 2>/dev/null
    fi
    cd /tmp/gasket-driver
    run_sudo make -C /lib/modules/$KERNEL/build M=$(pwd)/src modules 2>&1 | tail -5
    run_sudo make -C /lib/modules/$KERNEL/build M=$(pwd)/src modules_install 2>&1 | tail -3
    run_sudo depmod -a
}

# Load the module
run_sudo modprobe gasket 2>/dev/null || true
run_sudo modprobe apex 2>/dev/null || true

# Check if TPU device appeared
if [ -e /dev/apex_0 ]; then
    info "TPU device: /dev/apex_0 ✓"
    ls -la /dev/apex_0
else
    warn "TPU device /dev/apex_0 not present"
    warn "May need a reboot after gasket module install"
    # Add udev rule for permissions
    echo 'SUBSYSTEM=="apex", MODE="0660", GROUP="apex"' | run_sudo tee /etc/udev/rules.d/65-apex.rules > /dev/null
    run_sudo groupadd -f apex
    run_sudo usermod -aG apex cesarops
fi

echo ""

# ══════════════════════════════════════════════════════════════════════════════
# STEP 5: Pin nvidia-driver-580 to prevent auto-upgrade
# ══════════════════════════════════════════════════════════════════════════════
info "Step 5: Pinning nvidia-driver-580 to prevent future upgrades..."

run_sudo tee /etc/apt/preferences.d/nvidia-pin > /dev/null << 'EOF'
# Pin NVIDIA to 580.xx branch (Pascal P100 legacy)
# 595+ dropped Pascal support — DO NOT upgrade past 580
Package: nvidia-driver nvidia-driver-*
Pin: version 580.*
Pin-Priority: 1001

Package: nvidia-dkms nvidia-dkms-*
Pin: version 580.*
Pin-Priority: 1001

Package: libnvidia-* nvidia-kernel-* nvidia-firmware*
Pin: version 580.*
Pin-Priority: 1001
EOF

info "Pinned. apt will not upgrade past 580.xx"
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# SUMMARY
# ══════════════════════════════════════════════════════════════════════════════
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  DONE — Summary                                             ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║                                                              ║"
echo "║  External drive: $EXTERNAL (916GB)                ║"
echo "║  Models:         $MODELS_DIR        ║"
echo "║  HF Cache:       $CACHE_DIR/huggingface   ║"
echo "║  Ollama:         $CACHE_DIR/ollama        ║"
echo "║  Apt cache:      $APT_CACHE        ║"
echo "║                                                              ║"
echo "║  NVIDIA: 580.xx (Pascal legacy, pinned)                     ║"
echo "║  TPU:    gasket-dkms (slot 6)                                ║"
echo "║                                                              ║"
echo "║  REBOOT REQUIRED for:                                        ║"
echo "║    - NVIDIA 580 kernel module to load                        ║"
echo "║    - Coral TPU /dev/apex_0 to appear                         ║"
echo "║                                                              ║"
echo "║  After reboot, verify:                                       ║"
echo "║    nvidia-smi  (should show 2x P100)                         ║"
echo "║    ls /dev/apex_0  (TPU device)                              ║"
echo "║    sudo systemctl start koboldcpp                            ║"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "Reboot now? (y/n)"
read -r REPLY
if [[ "$REPLY" =~ ^[Yy]$ ]]; then
    run_sudo reboot
fi
