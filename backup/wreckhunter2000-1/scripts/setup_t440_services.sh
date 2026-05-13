#!/bin/bash
# ═══════════════════════════════════════════════════════════════════════════════
# T440 Full Service Setup — Cake + KoboldCPP + Ollama
# ═══════════════════════════════════════════════════════════════════════════════
# Run ON the T440 (ssh cesarops@100.72.182.77)
# This script installs and configures all three inference services.
#
# Services:
#   - cake.service       (port 5002) — Cake CUDA, Qwen3.6-35B-A3B MoE, distributed-capable
#   - koboldcpp.service  (port 5001) — KoboldCPP CUDA, GGUF models, fast single-node
#   - ollama.service     (port 11434) — Ollama, small utility models (7B)
#
# Only ONE of cake/koboldcpp should use the P100s at a time (32GB shared).
# Ollama runs CPU-only or on leftover VRAM with small models.
#
# Usage:
#   bash scripts/setup_t440_services.sh all       # install everything
#   bash scripts/setup_t440_services.sh cake      # cake only
#   bash scripts/setup_t440_services.sh kobold    # koboldcpp only
#   bash scripts/setup_t440_services.sh ollama    # ollama only
#   bash scripts/setup_t440_services.sh status    # check all services
# ═══════════════════════════════════════════════════════════════════════════════

set -euo pipefail

REPO="/home/cesarops/wreckhunter2000-1"
MODELS="/mnt/data-external/cesarops/models"
CAKE_DIR="/home/cesarops/cake"
STORY="$REPO/scripts/cesarops_story.json"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC} $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC} $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; }

# ── Credentials ───────────────────────────────────────────────────────────────
CRED_FILE="$REPO/scripts/credentials.sh"
if [ -f "$CRED_FILE" ]; then
    source "$CRED_FILE"
fi

run_sudo() {
    if [ -n "${SUDO_PASS:-}" ]; then
        echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null
    else
        sudo "$@"
    fi
}

# ═══════════════════════════════════════════════════════════════════════════════
# CAKE SETUP
# ═══════════════════════════════════════════════════════════════════════════════
setup_cake() {
    info "Setting up Cake (CUDA) for dual P100..."

    # Ensure Rust is available
    source "$HOME/.cargo/env" 2>/dev/null || true
    if ! command -v cargo &>/dev/null; then
        info "Installing Rust..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    fi

    # Clone or update Cake
    if [ -d "$CAKE_DIR" ]; then
        info "Updating Cake repo..."
        cd "$CAKE_DIR"
        git pull --ff-only || git fetch && git reset --hard origin/main
    else
        info "Cloning Cake..."
        git clone https://github.com/evilsocket/cake.git "$CAKE_DIR"
        cd "$CAKE_DIR"
    fi

    # Build with CUDA (P100s are Pascal — CUDA is 10-20x faster than Vulkan here)
    info "Building Cake with CUDA feature (this may take 5-10 min first time)..."
    cargo build --release --features cuda 2>&1 | tail -20

    CAKE_BIN="$CAKE_DIR/target/release/cake"
    if [ ! -f "$CAKE_BIN" ]; then
        error "Cake CUDA build failed. Trying Vulkan fallback..."
        cargo build --release --features vulkan 2>&1 | tail -20
    fi

    if [ ! -f "$CAKE_BIN" ]; then
        error "Cake build failed entirely. Check CUDA toolkit installation."
        return 1
    fi

    info "Cake binary: $(ls -lh $CAKE_BIN | awk '{print $5}')"

    # Pull the Qwen3.6-35B model (safetensors from HuggingFace)
    # Cake uses HF cache at ~/.cache/huggingface/hub/
    info "Pulling Qwen3-35B-A3B model (this will download ~20GB if not cached)..."
    info "Note: If the exact model ID differs, we'll try alternatives..."

    # Try the most likely HF model IDs for Qwen3.6-35B-A3B
    $CAKE_BIN pull Qwen/Qwen3-35B-A3B 2>&1 | tail -5 || \
    $CAKE_BIN pull Qwen/Qwen3.6-35B-A3B 2>&1 | tail -5 || \
    $CAKE_BIN pull evilsocket/Qwen3-35B-A3B 2>&1 | tail -5 || \
    warn "Auto-pull failed. You may need to find the correct HF model ID."
    warn "Try: $CAKE_BIN list   or check https://huggingface.co/Qwen"
    warn "Then: $CAKE_BIN pull <correct-model-id>"

    # Install systemd service
    info "Installing cake.service..."
    run_sudo tee /etc/systemd/system/cake.service > /dev/null << 'EOF'
[Unit]
Description=Cake LLM Inference Server (T440 dual P100 CUDA)
After=network.target
Wants=network.target
Conflicts=koboldcpp.service

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops/cake
ExecStart=/home/cesarops/cake/target/release/cake serve Qwen/Qwen3-35B-A3B \
    --port 5002 \
    --host 0.0.0.0
Restart=on-failure
RestartSec=10
StartLimitIntervalSec=120
StartLimitBurst=3
Environment="CUDA_VISIBLE_DEVICES=0,1"
Environment="HF_HOME=/home/cesarops/.cache/huggingface"
StandardOutput=journal
StandardError=journal
SyslogIdentifier=cake

[Install]
WantedBy=multi-user.target
EOF

    run_sudo systemctl daemon-reload
    run_sudo systemctl enable cake

    info "Cake installed. Start with: sudo systemctl start cake"
    info "Note: Conflicts with koboldcpp — only one can use P100s at a time."
    echo ""
}

# ═══════════════════════════════════════════════════════════════════════════════
# KOBOLDCPP SETUP
# ═══════════════════════════════════════════════════════════════════════════════
setup_kobold() {
    info "Setting up KoboldCPP for dual P100..."

    # Verify binary exists
    if [ ! -f "/home/cesarops/koboldcpp" ]; then
        warn "KoboldCPP binary not found at /home/cesarops/koboldcpp"
        warn "Download from: https://github.com/LostRuins/koboldcpp/releases"
        warn "Or build: make LLAMA_CUBLAS=1"
    fi

    # Verify model exists
    if [ ! -f "$MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf" ]; then
        warn "Model not found: $MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf"
        warn "Check /mnt/data-external/cesarops/models/ for available GGUFs"
    fi

    # Install systemd service
    info "Installing koboldcpp.service..."
    run_sudo tee /etc/systemd/system/koboldcpp.service > /dev/null << EOF
[Unit]
Description=KoboldCPP LLM Inference Server (T440 dual P100)
After=network.target
Wants=network.target
Conflicts=cake.service

[Service]
Type=simple
User=cesarops
WorkingDirectory=/home/cesarops
ExecStart=/home/cesarops/koboldcpp \\
    --model $MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \\
    --port 5001 \\
    --host 0.0.0.0 \\
    --gpulayers 64 \\
    --usecublas \\
    --contextsize 8192 \\
    --threads 16 \\
    --preloadstory $STORY
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

    run_sudo systemctl daemon-reload
    run_sudo systemctl enable koboldcpp

    info "KoboldCPP installed. Start with: sudo systemctl start koboldcpp"
    info "Note: Conflicts with cake — only one can use P100s at a time."
    echo ""
}

# ═══════════════════════════════════════════════════════════════════════════════
# OLLAMA SETUP
# ═══════════════════════════════════════════════════════════════════════════════
setup_ollama() {
    info "Setting up Ollama on T440..."

    # Install Ollama if not present
    if ! command -v ollama &>/dev/null; then
        info "Installing Ollama..."
        curl -fsSL https://ollama.com/install.sh | sh
    else
        info "Ollama already installed: $(which ollama)"
    fi

    # Configure Ollama to use CPU only (P100s reserved for Cake/KoboldCPP)
    # Or allow minimal VRAM usage for small models
    info "Installing ollama.service override..."
    run_sudo mkdir -p /etc/systemd/system/ollama.service.d
    run_sudo tee /etc/systemd/system/ollama.service.d/override.conf > /dev/null << 'EOF'
[Service]
Environment="OLLAMA_HOST=0.0.0.0:11434"
Environment="OLLAMA_NUM_PARALLEL=2"
Environment="OLLAMA_MAX_LOADED_MODELS=2"
# Limit GPU usage — leave VRAM for Cake/KoboldCPP
# Set CUDA_VISIBLE_DEVICES="" to force CPU-only
# Or leave it to share VRAM (small models fit alongside 35B)
Environment="OLLAMA_GPU_OVERHEAD=1073741824"
EOF

    run_sudo systemctl daemon-reload
    run_sudo systemctl enable ollama
    run_sudo systemctl start ollama

    # Wait for Ollama to be ready
    info "Waiting for Ollama to start..."
    for i in $(seq 1 15); do
        if curl -s http://localhost:11434/api/tags > /dev/null 2>&1; then
            break
        fi
        sleep 1
    done

    # Pull utility models
    info "Pulling utility models (qwen2.5:7b, qwen2.5-coder:7b)..."
    ollama pull qwen2.5:7b 2>&1 | tail -3 || warn "Failed to pull qwen2.5:7b"
    ollama pull qwen2.5-coder:7b 2>&1 | tail -3 || warn "Failed to pull qwen2.5-coder:7b"

    info "Ollama ready at http://0.0.0.0:11434"
    info "Models: qwen2.5:7b (reasoning), qwen2.5-coder:7b (coding)"
    echo ""
}

# ═══════════════════════════════════════════════════════════════════════════════
# STATUS CHECK
# ═══════════════════════════════════════════════════════════════════════════════
check_status() {
    echo "╔══════════════════════════════════════════════════════════╗"
    echo "║  T440 Service Status                                     ║"
    echo "╚══════════════════════════════════════════════════════════╝"
    echo ""

    for svc in cake koboldcpp ollama; do
        STATUS=$(systemctl is-active $svc 2>/dev/null || echo "not-found")
        ENABLED=$(systemctl is-enabled $svc 2>/dev/null || echo "not-found")
        case "$STATUS" in
            active)   echo -e "  ${GREEN}●${NC} $svc — running (enabled: $ENABLED)" ;;
            inactive) echo -e "  ${YELLOW}○${NC} $svc — stopped (enabled: $ENABLED)" ;;
            failed)   echo -e "  ${RED}✗${NC} $svc — failed (enabled: $ENABLED)" ;;
            *)        echo -e "  ${RED}?${NC} $svc — $STATUS" ;;
        esac
    done

    echo ""
    echo "  GPU Memory:"
    nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv,noheader 2>/dev/null || echo "  nvidia-smi not available"

    echo ""
    echo "  Endpoints:"
    echo "    Cake:      http://localhost:5002/v1  ($(curl -s --max-time 2 http://localhost:5002/v1/models > /dev/null 2>&1 && echo 'UP' || echo 'DOWN'))"
    echo "    KoboldCPP: http://localhost:5001/v1  ($(curl -s --max-time 2 http://localhost:5001/v1/models > /dev/null 2>&1 && echo 'UP' || echo 'DOWN'))"
    echo "    Ollama:    http://localhost:11434     ($(curl -s --max-time 2 http://localhost:11434/api/tags > /dev/null 2>&1 && echo 'UP' || echo 'DOWN'))"
    echo ""
}

# ═══════════════════════════════════════════════════════════════════════════════
# MAIN
# ═══════════════════════════════════════════════════════════════════════════════

case "${1:-}" in
    cake)
        setup_cake
        ;;
    kobold|koboldcpp)
        setup_kobold
        ;;
    ollama)
        setup_ollama
        ;;
    all)
        setup_cake
        setup_kobold
        setup_ollama
        echo ""
        info "All services installed."
        info ""
        info "IMPORTANT: Cake and KoboldCPP conflict (both want P100 VRAM)."
        info "Start ONE of them, plus Ollama:"
        info ""
        info "  Option A (Cake + Ollama):"
        info "    sudo systemctl start cake ollama"
        info ""
        info "  Option B (KoboldCPP + Ollama):"
        info "    sudo systemctl start koboldcpp ollama"
        info ""
        info "Switch between them:"
        info "    sudo systemctl stop cake && sudo systemctl start koboldcpp"
        info "    sudo systemctl stop koboldcpp && sudo systemctl start cake"
        ;;
    status|--status|-s)
        check_status
        ;;
    *)
        echo "Usage: bash scripts/setup_t440_services.sh <command>"
        echo ""
        echo "Commands:"
        echo "  all      — Install Cake + KoboldCPP + Ollama"
        echo "  cake     — Install Cake (CUDA, distributed-capable)"
        echo "  kobold   — Install KoboldCPP (GGUF, fast single-node)"
        echo "  ollama   — Install Ollama (small utility models)"
        echo "  status   — Check all service status"
        exit 1
        ;;
esac
