#!/bin/bash
# Xenon Server Setup Script
# Installs Coral TPU drivers, Python deps, and configures services

set -e

echo "========================================"
echo "CESAROPS XENON SETUP"
echo "========================================"
echo ""

# Check if running as root
if [ "$EUID" -ne 0 ]; then 
    echo "Please run with sudo"
    exit 1
fi

echo "[1/5] Updating system..."
apt update -qq

echo "[2/5] Installing Coral TPU drivers..."
# Add Google Coral repo
curl -s https://packages.cloud.google.com/apt/doc/apt-key.gpg | apt-key add -qq
echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" > /etc/apt/sources.list.d/coral-edgetpu.list

# Install TPU library (PCIe and USB support)
apt update -qq
apt install -y libedgetpu1-std pciutils

echo "[3/5] Checking for Coral TPU devices..."
# Check PCIe
if lspci -d 1a6e: > /dev/null 2>&1; then
    echo "  ✓ Found Coral PCIe device:"
    lspci -d 1a6e: | sed 's/^/    /'
else
    echo "  ⚠ No Coral PCIe device found"
fi

# Check USB
if lsusb -d 1a6e: > /dev/null 2>&1; then
    echo "  ✓ Found Coral USB device:"
    lsusb -d 1a6e: | sed 's/^/    /'
else
    echo "  ⚠ No Coral USB device found"
fi

echo "[3/5] Installing Python dependencies..."
pip3 install flask pillow numpy requests

# Optional: CuPy for GPU (if CUDA installed)
if command -v nvcc &> /dev/null; then
    echo "  CUDA detected, installing CuPy..."
    pip3 install cupy-cuda11x
else
    echo "  CUDA not found, skipping CuPy"
fi

echo "[4/5] Checking USB devices..."
lsusb | grep -q "Google" && echo "  ✓ Coral USB detected" || echo "  ⚠ Coral USB not found (plug it in)"

echo "[5/5] Creating systemd services..."

# TPU Server service
cat > /etc/systemd/system/cesarops-tpu.service << EOF
[Unit]
Description=CESAROPS TPU Server
After=network.target

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops/cesarops-wreckhunter-build
ExecStart=/usr/bin/python3 /home/cesarops/cesarops-wreckhunter-build/tpu_server.py
Restart=always
RestartSec=10

[Install]
WantedBy=multi-user.target
EOF

# Xenon Engine service
cat > /etc/systemd/system/cesarops-engine.service << EOF
[Unit]
Description=CESAROPS Processing Engine
After=network.target cesarops-tpu.service

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops/cesarops-wreckhunter-build
ExecStart=/usr/bin/python3 /home/cesarops/cesarops-wreckhunter-build/cesarops_engine.py
Restart=always
RestartSec=30

[Install]
WantedBy=multi-user.target
EOF

# Enable services
systemctl daemon-reload
systemctl enable cesarops-tpu.service
systemctl enable cesarops-engine.service

echo ""
echo "========================================"
echo "SETUP COMPLETE!"
echo "========================================"
echo ""
echo "Services created:"
echo "  - cesarops-tpu (TPU server on port 5001)"
echo "  - cesarops-engine (main processing)"
echo ""
echo "Commands:"
echo "  sudo systemctl start cesarops-tpu"
echo "  sudo systemctl start cesarops-engine"
echo "  sudo systemctl status cesarops-tpu"
echo ""
echo "Test TPU server:"
echo "  curl http://localhost:5001/health"
echo ""
echo "========================================"
