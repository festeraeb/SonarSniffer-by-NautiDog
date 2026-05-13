#!/bin/bash
# CESAROPS Model Swap Script
# ===========================
# Run ON the T440 to swap KoboldCPP to a different model + story config.
# Requires sudo (will prompt for password).
#
# Usage:
#   bash scripts/swap_model.sh qwen36       # Qwen3.6-35B (research/coding)
#   bash scripts/swap_model.sh coder14      # Qwen2.5-Coder-14B (fast coding)
#   bash scripts/swap_model.sh rustcoder    # Rust-Coder-14B (Rust specialist)
#
# What it does:
#   1. Stops the koboldcpp systemd service
#   2. Waits for GPU memory to free
#   3. Writes a new service file for the selected model
#   4. Reloads systemd and starts the new config
#   5. Waits for the model to load and confirms it's serving

set -euo pipefail

REPO="/home/cesarops/wreckhunter2000-1"
MODELS="/mnt/data-external/cesarops/models"
STORY="$REPO/scripts/cesarops_story.json"
SERVICE="/etc/systemd/system/koboldcpp.service"

# ── Load credentials ──────────────────────────────────────────────────────────
CRED_FILE="$REPO/scripts/credentials.sh"
if [ ! -f "$CRED_FILE" ]; then
    echo "ERROR: Credentials file not found: $CRED_FILE"
    echo "Create it with: echo 'SUDO_PASS=\"yourpassword\"' > $CRED_FILE"
    exit 1
fi
source "$CRED_FILE"

# Helper: run sudo without prompting
run_sudo() {
    echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null
}

# ── Model presets ──────────────────────────────────────────────────────────────

case "${1:-}" in
    qwen36|qwen3.6|research|big)
        MODEL_FILE="$MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf"
        GPU_LAYERS=64
        CTX_SIZE=8192
        LABEL="Qwen3.6-35B-A3B (research/coding)"
        EXTRA_FLAGS="--preloadstory $STORY"
        ;;
    coder14|coder|fast)
        MODEL_FILE="$MODELS/qwen2.5-coder-14b-instruct-q4_k_m.gguf"
        GPU_LAYERS=48
        CTX_SIZE=4096
        LABEL="Qwen2.5-Coder-14B (fast coding)"
        EXTRA_FLAGS="--preloadstory $STORY --multiuser 4"
        ;;
    rustcoder|rust)
        MODEL_FILE="$MODELS/Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K.gguf"
        GPU_LAYERS=48
        CTX_SIZE=4096
        LABEL="Rust-Coder-14B (Rust specialist)"
        EXTRA_FLAGS="--preloadstory $STORY"
        ;;
    list|--list|-l)
        echo "Available model presets:"
        echo "  qwen36    — Qwen3.6-35B-A3B MXFP4 MoE (20GB, research/coding, 8K ctx)"
        echo "  coder14   — Qwen2.5-Coder-14B Q4_K_M (8GB, fast coding, 4K ctx)"
        echo "  rustcoder — Rust-Coder-14B Q6_K (10GB, Rust specialist, 4K ctx)"
        exit 0
        ;;
    *)
        echo "Usage: bash scripts/swap_model.sh <preset>"
        echo ""
        echo "Presets: qwen36 | coder14 | rustcoder"
        echo "Run with --list for details"
        exit 1
        ;;
esac

# ── Verify model file exists ──────────────────────────────────────────────────

if [ ! -f "$MODEL_FILE" ]; then
    echo "ERROR: Model file not found: $MODEL_FILE"
    exit 1
fi

echo "╔══════════════════════════════════════════════════════════╗"
echo "║  CESAROPS Model Swap                                     ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Target: $LABEL"
echo "  Model:  $MODEL_FILE"
echo "  Layers: $GPU_LAYERS | Context: $CTX_SIZE"
echo "  Story:  $STORY"
echo ""

# ── Step 1: Stop current service ──────────────────────────────────────────────

echo "[1/5] Stopping koboldcpp service..."
run_sudo systemctl stop koboldcpp 2>/dev/null || true
sleep 3

# Verify it's dead
if pgrep -f koboldcpp > /dev/null 2>&1; then
    echo "  Still running — force killing..."
    run_sudo pkill -9 -f koboldcpp 2>/dev/null || true
    sleep 2
fi
echo "  ✓ Stopped"

# ── Step 2: Write new service file ────────────────────────────────────────────

echo "[2/5] Writing service file..."
cat << EOF | run_sudo tee "$SERVICE" > /dev/null
[Unit]
Description=KoboldCPP LLM Inference Server (T440 dual P100)
After=network.target
Wants=network.target

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops
ExecStart=/home/cesarops/koboldcpp \\
    --model $MODEL_FILE \\
    --port 5001 \\
    --host 0.0.0.0 \\
    --gpulayers $GPU_LAYERS \\
    --usecublas \\
    --contextsize $CTX_SIZE \\
    --threads 16 \\
    $EXTRA_FLAGS
Restart=always
RestartSec=10
StartLimitIntervalSec=120
StartLimitBurst=3
StandardOutput=journal
StandardError=journal
SyslogIdentifier=koboldcpp

[Install]
WantedBy=multi-user.target
EOF
echo "  ✓ Written"

# ── Step 3: Reload systemd ────────────────────────────────────────────────────

echo "[3/5] Reloading systemd..."
run_sudo systemctl daemon-reload
echo "  ✓ Reloaded"

# ── Step 4: Start new service ─────────────────────────────────────────────────

echo "[4/5] Starting koboldcpp with $LABEL..."
run_sudo systemctl start koboldcpp
echo "  ✓ Started"

# ── Step 5: Wait for model to load ────────────────────────────────────────────

echo "[5/5] Waiting for model to load (up to 90s)..."
for i in $(seq 1 18); do
    sleep 5
    if curl -s --max-time 3 http://localhost:5001/v1/models > /dev/null 2>&1; then
        MODEL_ID=$(curl -s http://localhost:5001/v1/models | python3 -c "import sys,json; print(json.load(sys.stdin)['data'][0]['id'])" 2>/dev/null || echo "unknown")
        echo ""
        echo "  ✓ Model loaded: $MODEL_ID"
        echo ""
        echo "╔══════════════════════════════════════════════════════════╗"
        echo "║  READY — $LABEL"
        echo "║  Endpoint: http://100.72.182.77:5001/v1                  ║"
        echo "╚══════════════════════════════════════════════════════════╝"
        exit 0
    fi
    printf "."
done

echo ""
echo "WARNING: Model did not respond within 90s. Check logs:"
echo "  sudo journalctl -u koboldcpp -n 20 --no-pager"
exit 1
