#!/usr/bin/env bash
# Download Qwen2.5-Coder-32B-Instruct Q4_K_M to T440
# Run this ON the T440: bash scripts/download_32b_t440.sh
#
# File size: ~18GB — needs ~20GB free on /home/cesarops/models/
# Estimated download time: 10-30 min depending on connection

set -e

MODEL_DIR="/home/cesarops/models"
MODEL_FILE="qwen2.5-coder-32b-instruct-q4_k_m.gguf"
HF_URL="https://huggingface.co/Qwen/Qwen2.5-Coder-32B-Instruct-GGUF/resolve/main/qwen2.5-coder-32b-instruct-q4_k_m.gguf"

echo "╔══════════════════════════════════════════════════════════╗"
echo "║  Downloading Qwen2.5-Coder-32B Q4_K_M → T440            ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Destination: $MODEL_DIR/$MODEL_FILE"
echo "  Size: ~18 GB"
echo ""

# Check disk space
AVAIL=$(df -BG "$MODEL_DIR" | awk 'NR==2 {print $4}' | tr -d 'G')
if [ "$AVAIL" -lt 20 ]; then
    echo "✗ Not enough disk space. Need 20GB, have ${AVAIL}GB."
    exit 1
fi
echo "  Disk space: ${AVAIL}GB available ✓"
echo ""

# Install huggingface-cli if not present
if ! command -v huggingface-cli &>/dev/null; then
    echo "Installing huggingface-hub..."
    pip install huggingface-hub --quiet
fi

# Download with resume support
echo "Starting download..."
huggingface-cli download \
    Qwen/Qwen2.5-Coder-32B-Instruct-GGUF \
    "$MODEL_FILE" \
    --local-dir "$MODEL_DIR" \
    --local-dir-use-symlinks False

echo ""
echo "✓ Downloaded: $MODEL_DIR/$MODEL_FILE"
echo ""
echo "To activate the dual-P100 32B preset:"
echo "  sudo cp scripts/koboldcpp-dual-p100-32b.service /etc/systemd/system/koboldcpp.service"
echo "  sudo systemctl daemon-reload && sudo systemctl restart koboldcpp"
