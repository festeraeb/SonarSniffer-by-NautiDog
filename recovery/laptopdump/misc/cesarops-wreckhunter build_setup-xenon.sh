#!/bin/bash
# CESAROPS Quick Start Script for Xenon
# Run this after booting to test everything

echo "=================================="
echo "CESAROPS - Xenon Quick Test"
echo "=================================="
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo "Please run with sudo"
    exit 1
fi

# Apply network config
echo "1. Applying network config..."
netplan apply
sleep 2

# Test network
echo "2. Testing network..."
ping -c 3 google.com
echo ""

# Check GPU (if CUDA installed)
echo "3. Checking GPU..."
if command -v nvidia-smi &> /dev/null; then
    nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv
else
    echo "   CUDA not installed yet - run: apt install cuda-drivers-525"
fi
echo ""

# Check Coral TPU
echo "4. Checking Coral TPU..."
if ls /dev/apex* &> /dev/null; then
    echo "   ✓ Coral TPU detected"
else
    echo "   Coral TPU not installed yet"
fi
echo ""

# Test CESAROPS
echo "5. Testing CESAROPS..."
/home/lucky/cesarops-search --version
echo ""

echo "=================================="
echo "Setup Complete!"
echo "=================================="
echo ""
echo "Next steps:"
echo "  1. Install CUDA: apt install cuda-drivers-525"
echo "  2. Install Coral: apt install libedgetpu1-std"
echo "  3. Run scan: /home/lucky/cesarops-search scan michigan --input ./data --output ./outputs"
echo ""
