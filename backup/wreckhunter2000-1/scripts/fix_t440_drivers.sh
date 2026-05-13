#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# T440 Fix: Mount external drive, install nvidia-580, fix Coral TPU
# ═══════════════════════════════════════════════════════════════════════════════
# Run ON the T440 as cesarops (will use sudo)
set -euo pipefail

SUDO_PASS="cesarops"
run_sudo() { echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null; }

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  T440 Driver Fix + Storage Migration                         ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# ══════════════════════════════════════════════════════════════════════════════
# STEP 1: Persistent mount for external drive
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[1/6] Setting up persistent mount for /mnt/data-external..."

run_sudo mkdir -p /mnt/data-external
if ! grep -q "wreckhunter-data" /etc/fstab; then
    echo 'UUID=dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9 /mnt/data-external ext4 defaults,nofail 0 2' | run_sudo tee -a /etc/fstab > /dev/null
    echo "  ✓ Added to fstab"
else
    echo "  ✓ Already in fstab"
fi

# Ensure it's mounted now
if ! mountpoint -q /mnt/data-external; then
    run_sudo mount /mnt/data-external
fi
echo "  ✓ Mounted: $(df -h /mnt/data-external | tail -1 | awk '{print $4 " free"}')"

# ══════════════════════════════════════════════════════════════════════════════
# STEP 2: Move heavy directories to external drive
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[2/6] Moving heavy data to /mnt/data-external..."

# Create cesarops directory structure on external
run_sudo mkdir -p /mnt/data-external/cesarops/{models,cache,benchmark,apt-cache}
run_sudo chown -R cesarops:cesarops /mnt/data-external/cesarops

# Move apt cache to external
run_sudo rm -rf /var/cache/apt/archives/*.deb
run_sudo ln -sfn /mnt/data-external/cesarops/apt-cache /var/cache/apt/archives || true

# Move ~/.cache to external (huggingface models, pip cache, etc)
if [ -d "$HOME/.cache" ] && [ ! -L "$HOME/.cache" ]; then
    echo "  Moving ~/.cache ($(du -sh ~/.cache 2>/dev/null | cut -f1))..."
    rsync -a --remove-source-files "$HOME/.cache/" /mnt/data-external/cesarops/cache/ 2>/dev/null || true
    rm -rf "$HOME/.cache"
    ln -sf /mnt/data-external/cesarops/cache "$HOME/.cache"
    echo "  ✓ ~/.cache → external"
fi

# Move benchmark dir if it exists
if [ -d "$HOME/benchmark" ] && [ ! -L "$HOME/benchmark" ]; then
    echo "  Moving ~/benchmark..."
    mv "$HOME/benchmark" /mnt/data-external/cesarops/benchmark 2>/dev/null || true
    ln -sf /mnt/data-external/cesarops/benchmark "$HOME/benchmark"
    echo "  ✓ ~/benchmark → external"
fi

# Move ~/.local (pip packages, etc)
if [ -d "$HOME/.local" ] && [ ! -L "$HOME/.local" ]; then
    echo "  Moving ~/.local ($(du -sh ~/.local 2>/dev/null | cut -f1))..."
    rsync -a "$HOME/.local/" /mnt/data-external/cesarops/local/ 2>/dev/null || true
    rm -rf "$HOME/.local"
    ln -sf /mnt/data-external/cesarops/local "$HOME/.local"
    echo "  ✓ ~/.local → external"
fi

echo "  Root disk after cleanup:"
df -h / | tail -1 | awk '{print "    " $3 " used / " $2 " total (" $5 " full)"}'

# ══════════════════════════════════════════════════════════════════════════════
# STEP 3: Clean apt and remove broken gasket-dkms
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[3/6] Cleaning apt and removing broken gasket-dkms..."

run_sudo apt-get clean
run_sudo dpkg --configure -a 2>/dev/null || true
run_sudo apt-get remove -y gasket-dkms 2>/dev/null || true
run_sudo apt-get autoremove -y 2>/dev/null || true

echo "  ✓ Cleaned"
df -h / | tail -1 | awk '{print "    Root: " $4 " free"}'

# ══════════════════════════════════════════════════════════════════════════════
# STEP 4: Install NVIDIA 580 driver (Pascal legacy branch)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[4/6] Installing nvidia-driver-580 (Pascal P100 legacy branch)..."
echo "  Removing 595.71 (too new for P100)..."

run_sudo apt-get remove -y nvidia-driver 2>/dev/null || true
run_sudo apt-get install -y nvidia-driver-580 2>&1 | tail -10

# Verify the module is built for current kernel
KVER=$(uname -r)
if [ -f "/lib/modules/$KVER/updates/dkms/nvidia.ko.zst" ] || [ -f "/lib/modules/$KVER/updates/dkms/nvidia.ko" ]; then
    MODVER=$(modinfo nvidia 2>/dev/null | grep "^version:" | awk '{print $2}')
    echo "  ✓ nvidia module built: $MODVER for kernel $KVER"
else
    echo "  ✗ Module not found for $KVER — may need dkms build"
    run_sudo dkms autoinstall 2>&1 | tail -5
fi

# Load the module
run_sudo modprobe nvidia 2>&1 || echo "  (will work after reboot)"

# Test
if nvidia-smi > /dev/null 2>&1; then
    echo "  ✓ nvidia-smi working!"
    nvidia-smi --query-gpu=index,name,driver_version,memory.total --format=csv,noheader
else
    echo "  ⚠ nvidia-smi not working yet — reboot required"
fi

# ══════════════════════════════════════════════════════════════════════════════
# STEP 5: Install Coral Edge TPU driver (gasket + apex)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[5/6] Setting up Coral Edge TPU (PCIe slot 6)..."

# The Coral PCIe TPU needs gasket-dkms + libedgetpu
# gasket-dkms was broken — reinstall from Google's repo

# Add Google Coral repo if not present
if [ ! -f /etc/apt/sources.list.d/coral-edgetpu.list ]; then
    echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" | run_sudo tee /etc/apt/sources.list.d/coral-edgetpu.list > /dev/null
    curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | run_sudo apt-key add - 2>/dev/null
    run_sudo apt-get update -qq
fi

# Install gasket driver (kernel module for PCIe TPU)
echo "  Installing gasket-dkms..."
run_sudo apt-get install -y gasket-dkms 2>&1 | tail -5

# Install libedgetpu (runtime library)
echo "  Installing libedgetpu..."
run_sudo apt-get install -y libedgetpu1-std 2>&1 | tail -3

# Load the module
run_sudo modprobe apex 2>&1 || run_sudo modprobe gasket 2>&1 || true

# Add udev rule for non-root access
if [ ! -f /etc/udev/rules.d/65-apex.rules ]; then
    echo 'SUBSYSTEM=="apex", MODE="0660", GROUP="apex"' | run_sudo tee /etc/udev/rules.d/65-apex.rules > /dev/null
    run_sudo groupadd -f apex
    run_sudo usermod -aG apex cesarops
    run_sudo udevadm control --reload-rules
    run_sudo udevadm trigger
    echo "  ✓ udev rules installed"
fi

# Check if TPU is visible
if ls /dev/apex_0 > /dev/null 2>&1; then
    echo "  ✓ Coral TPU available at /dev/apex_0"
else
    echo "  ⚠ /dev/apex_0 not present — may need reboot after gasket module loads"
    echo "    PCIe device: $(lspci | grep -i 'coral\|apex\|google')"
fi

# ══════════════════════════════════════════════════════════════════════════════
# STEP 6: Pin nvidia-driver-580 to prevent auto-upgrade
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[6/6] Pinning nvidia-driver-580 to prevent auto-upgrade..."

run_sudo tee /etc/apt/preferences.d/nvidia-pin > /dev/null << 'EOF'
# Pin NVIDIA to 580.xx legacy branch (Pascal P100 support)
# 595+ dropped Pascal — do NOT upgrade past 580
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
echo "  ✓ Pinned to 580.xx — apt won't auto-upgrade to 595+"

# ══════════════════════════════════════════════════════════════════════════════
# SUMMARY
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo ""
echo "  Storage:"
echo "    Root:     $(df -h / | tail -1 | awk '{print $4}') free"
echo "    External: $(df -h /mnt/data-external | tail -1 | awk '{print $4}') free"
echo ""
echo "  GPU: $(nvidia-smi --query-gpu=name,driver_version --format=csv,noheader 2>/dev/null || echo 'needs reboot')"
echo "  TPU: $(ls /dev/apex_0 2>/dev/null && echo 'OK' || echo 'needs reboot')"
echo ""
echo "  If nvidia-smi or TPU not working, reboot:"
echo "    sudo reboot"
echo ""
echo "  After reboot, start services:"
echo "    sudo systemctl start koboldcpp ollama"
echo ""
echo "═══════════════════════════════════════════════════════════════════"
