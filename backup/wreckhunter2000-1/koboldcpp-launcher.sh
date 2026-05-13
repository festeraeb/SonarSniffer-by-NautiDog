#!/bin/bash
# koboldcpp-launcher.sh
# Deploy and manage KoboldCPP on local machine
# Run this directly on Xeon or p1000

set -e

MACHINE_NAME=${1:-"Unknown"}
MODEL_PATH=${2:-"/models/llama3-8b.gguf"}
PORT=${3:-5001}
GPU_LAYERS=${4:-11}

# Detect koboldcpp location
if [ -f "$HOME/ai_coding/koboldcpp" ]; then
    KOBOLD_BIN="$HOME/ai_coding/koboldcpp"
elif [ -f "/opt/koboldcpp/koboldcpp-linux-x64" ]; then
    KOBOLD_BIN="/opt/koboldcpp/koboldcpp-linux-x64"
else
    echo "❌ KoboldCPP not found!"
    exit 1
fi

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║ KoboldCPP LAUNCHER - $MACHINE_NAME"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""
echo "📦 Binary: $KOBOLD_BIN"
echo "🎯 Model: $MODEL_PATH"
echo "🔌 Port: $PORT"
echo "⚡ GPU Layers: $GPU_LAYERS"
echo ""

# Stop any existing instance
pkill -f "koboldcpp.*--port $PORT" 2>/dev/null || true
sleep 1

# Start KoboldCPP
echo "🚀 Starting KoboldCPP..."
nohup "$KOBOLD_BIN" \
    --model "$MODEL_PATH" \
    --port "$PORT" \
    --gpulayers "$GPU_LAYERS" \
    --contextsize 4096 \
    --threads 8 \
    > "$HOME/koboldcpp-$PORT.log" 2>&1 &

PID=$!
echo "✓ Process started (PID: $PID)"

# Wait for startup
sleep 5

# Check if running
if pgrep -f "koboldcpp.*--port $PORT" > /dev/null; then
    echo ""
    echo "╔════════════════════════════════════════════════════════════════╗"
    echo "║ ✅ KoboldCPP is RUNNING                                        ║"
    echo "╠════════════════════════════════════════════════════════════════╣"
    echo "║                                                                ║"
    echo "║  API Endpoint: http://localhost:$PORT/api/v1                  ║"
    echo "║  Models:       http://localhost:$PORT/api/v1/models           ║"
    echo "║  Logs:         tail -f ~/koboldcpp-$PORT.log                  ║"
    echo "║                                                                ║"
    echo "╚════════════════════════════════════════════════════════════════╝"
else
    echo ""
    echo "⚠️  Process may have failed. Check log:"
    echo "   tail -f ~/koboldcpp-$PORT.log"
    exit 1
fi
