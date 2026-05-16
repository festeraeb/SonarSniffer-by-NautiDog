#!/usr/bin/env bash
# ============================================================
# Launch koboldcpp on ThinkPad with Quadro M2200 (4GB Maxwell)
# Run this ON THE LAPTOP (Windows: use WSL2 or Git Bash)
# or SSH in if OpenSSH server is enabled on Windows.
#
# M2200 specs: Maxwell GM206, 4GB GDDR5, CUDA 5.0
# Fits: TinyLlama 1.1B Q4_K_M (638MB) — validator/corrector role
#       Phi-3-mini Q4_K_M (2.3GB) — light coder role
#       DeepSeek-R1-7B Q2_K (2.9GB) — reasoning role (tight)
#
# Tailscale IP: 100.110.214.86
# Target port: 5571 (validator) or 5572 (coder)
# ============================================================

set -e

KOBOLD_BIN="${KOBOLD_BIN:-./koboldcpp}"
MODEL_DIR="${MODEL_DIR:-/mnt/c/models}"  # WSL2 path — adjust for native Windows
PORT="${1:-5571}"
ROLE="${2:-validator}"  # validator | coder | reasoner

case "$ROLE" in
  validator)
    MODEL="$MODEL_DIR/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf"
    GPU_LAYERS=999
    CTX=2048
    THREADS=2
    ;;
  coder)
    MODEL="$MODEL_DIR/Phi-3-mini-4k-instruct-Q4_K_M.gguf"
    GPU_LAYERS=999
    CTX=4096
    THREADS=4
    ;;
  reasoner)
    MODEL="$MODEL_DIR/DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q2_K.gguf"
    GPU_LAYERS=28   # partial offload — 4GB is tight for 7B Q2
    CTX=2048
    THREADS=4
    ;;
  *)
    echo "Unknown role: $ROLE. Use: validator | coder | reasoner"
    exit 1
    ;;
esac

echo "╔══════════════════════════════════════════════════════╗"
echo "║  ThinkPad M2200 — koboldcpp launcher                 ║"
echo "╠══════════════════════════════════════════════════════╣"
echo "║  Role:   $ROLE"
echo "║  Model:  $MODEL"
echo "║  Port:   $PORT"
echo "║  GPU:    Quadro M2200 (Maxwell, CUDA 5.0, 4GB)"
echo "╚══════════════════════════════════════════════════════╝"

# Check model exists
if [ ! -f "$MODEL" ]; then
    echo "ERROR: Model not found at $MODEL"
    echo "Copy models from T440: scp cesarops@100.72.182.77:/codebase/models/TinyLlama-1.1B-Chat-v1.0-Q4_K_M.gguf $MODEL_DIR/"
    exit 1
fi

# Kill any existing instance on this port
pkill -f "koboldcpp.*--port $PORT" 2>/dev/null || true
sleep 1

# Launch
echo "Starting koboldcpp..."
"$KOBOLD_BIN" \
    --model "$MODEL" \
    --port "$PORT" \
    --usecuda \
    --gpulayers "$GPU_LAYERS" \
    --contextsize "$CTX" \
    --threads "$THREADS" \
    --quiet \
    --host 0.0.0.0 \
    > ~/koboldcpp-$PORT.log 2>&1 &

PID=$!
echo "PID: $PID"
sleep 8

# Verify
if curl -s --max-time 5 "http://localhost:$PORT/v1/models" | grep -q "koboldcpp"; then
    echo ""
    echo "✅ koboldcpp running on :$PORT"
    echo "   Tailscale endpoint: http://100.110.214.86:$PORT"
    echo "   Add to forge cluster_config.toml:"
    echo "   [[agent]]"
    echo "   name = \"laptop-$ROLE\""
    echo "   endpoint = \"http://100.110.214.86:$PORT\""
    echo "   hardware = \"ThinkPad M2200 4GB\""
    echo "   role = \"$ROLE\""
else
    echo "⚠️  koboldcpp may not be ready yet. Check: tail -f ~/koboldcpp-$PORT.log"
fi
