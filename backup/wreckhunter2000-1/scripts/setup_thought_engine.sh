#!/usr/bin/env bash
# setup_thought_engine.sh
# Runs on cesarops2 (100.102.158.111)
# Downloads Qwen3-8B, replaces the 3B model, sets up the Thought Engine daemon.

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CESAROPS2_HOME="/home/cesarops" # Adjust if needed
KOBOLD_CPP_BIN="${CESAROPS2_HOME}/benchmark/koboldcpp"
MODELS_DIR="${CESAROPS2_HOME}/benchmark/models"
VENV_DIR="${CESAROPS2_HOME}/thought_engine_venv"
MODEL_NAME="Qwen3-8B-Q4_K_M.gguf"
MODEL_URL="https://huggingface.co/bartowski/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf"
KOBOLD_PORT=5555
THOUGHT_ENGINE_PORT=5556

echo "=== CESAROPS Thought Engine Setup ==="
echo "Target: cesarops2 (${CESAROPS2_HOME})"
echo "Model: ${MODEL_NAME}"

# 1. Create Python venv and install deps
echo "[1/5] Setting up Python environment..."
if [ ! -d "${VENV_DIR}" ]; then
    python3 -m venv "${VENV_DIR}"
fi
source "${VENV_DIR}/bin/activate"
pip install --upgrade pip
pip install huggingface-hub httpx fastapi uvicorn pydantic

# 2. Download Model
echo "[2/5] Downloading Qwen3-8B-Q4_K_M..."
MODEL_PATH="${MODELS_DIR}/${MODEL_NAME}"
if [ ! -f "${MODEL_PATH}" ]; then
    echo "  Downloading via huggingface-cli..."
    huggingface-cli download bartowski/Qwen3-8B-GGUF \
        --filename Qwen3-8B-Q4_K_M.gguf \
        --local-dir "${MODELS_DIR}"
else
    echo "  Model already exists at ${MODEL_PATH}. Skipping download."
fi

# 3. Kill existing KoboldCPP (3B model)
echo "[3/5] Stopping existing KoboldCPP processes..."
pkill -f "koboldcpp.*3B" || true
sleep 2

# 4. Start KoboldCPP with Qwen3-8B on port 5555
echo "[4/5] Starting KoboldCPP with Qwen3-8B on port ${KOBOLD_PORT}..."
nohup "${KOBOLD_CPP_BIN}" \
    --model "${MODEL_PATH}" \
    --port "${KOBOLD_PORT}" \
    --gpulayers 40 \
    --contextsize 8192 \
    --threads 8 \
    --log-disable \
    > /var/log/kobold-qwen3-8b.log 2>&1 &
KOBOLD_PID=$!
echo "  KoboldCPP started (PID ${KOBOLD_PID})"

# Wait for Kobold to be ready
echo "  Waiting for KoboldCPP to initialize..."
for i in {1..30}; do
    if curl -s http://localhost:${KOBOLD_PORT}/health > /dev/null 2>&1; then
        echo "  KoboldCPP is ready."
        break
    fi
    sleep 1
done

# 5. Install systemd service for Thought Engine Daemon
echo "[5/5] Installing Thought Engine systemd service..."
cp "${REPO_DIR}/scripts/thought-engine.service" /etc/systemd/system/thought-engine.service
systemctl daemon-reload
systemctl enable thought-engine.service
systemctl start thought-engine.service

echo ""
echo "=== Setup Complete ==="
echo "KoboldCPP (Qwen3-8B) running on port ${KOBOLD_PORT}"
echo "Thought Engine Daemon running on port ${THOUGHT_ENGINE_PORT}"
echo "Check status: systemctl status thought-engine"
