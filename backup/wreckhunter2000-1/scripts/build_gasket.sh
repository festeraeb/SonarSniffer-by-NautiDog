#!/bin/bash
# Build gasket/apex kernel module from source for current kernel
set -e

KERNEL=$(uname -r)
echo "Building gasket driver for kernel: $KERNEL"

# Install build deps
sudo apt-get install -y linux-headers-$KERNEL build-essential dkms git 2>&1 | tail -5

# Clone gasket driver
cd /tmp
rm -rf gasket-driver
git clone https://github.com/google/gasket-driver.git
cd gasket-driver/src

# Build
echo "Compiling..."
make -C /lib/modules/$KERNEL/build M=$(pwd) modules 2>&1 | tail -10

# Install
echo "Installing modules..."
sudo cp gasket.ko apex.ko /lib/modules/$KERNEL/extra/ 2>/dev/null || \
sudo mkdir -p /lib/modules/$KERNEL/extra && sudo cp gasket.ko apex.ko /lib/modules/$KERNEL/extra/
sudo depmod -a

# Load
echo "Loading modules..."
sudo modprobe gasket
sudo modprobe apex

# Verify
echo ""
if ls /dev/apex_0 2>/dev/null; then
    echo "✓ Coral TPU online at /dev/apex_0"
    # Set permissions
    sudo chmod 666 /dev/apex_0
else
    echo "✗ /dev/apex_0 not found"
    dmesg | grep -i "apex\|gasket" | tail -5
fi
