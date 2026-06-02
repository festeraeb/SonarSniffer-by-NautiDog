#!/usr/bin/env bash
# Launch llama-server on a remote GPU node (cesarops2 / cesarops3).
# M2200 (Maxwell) stays on launch_laptop_m2200.sh + Kobold.
#
# Usage:
#   MODEL=/path/to/model.gguf PORT=5570 DEV=CUDA0 CTX=4096 \
#     bash launch_llama_remote.sh
#
set -euo pipefail

LLAMA_BIN="${LLAMA_BIN:-${HOME}/llama.cpp/build/bin/llama-server}"
MODEL="${MODEL:?MODEL required}"
PORT="${PORT:?PORT required}"
HOST="${HOST:-0.0.0.0}"
DEV="${DEV:-CUDA0}"
CTX="${CTX:-8192}"
THREADS="${THREADS:-4}"
NGL="${NGL:-99}"
LOG="${LOG:-/tmp/llama-server-${PORT}.log}"

if [[ ! -x "$LLAMA_BIN" ]]; then
  echo "llama-server not found: $LLAMA_BIN"
  echo "Build on this host: git clone https://github.com/ggml-org/llama.cpp && cmake -B build -DGGML_CUDA=ON && cmake --build build -j"
  exit 1
fi

pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
pkill -f "koboldcpp.*--port ${PORT}" 2>/dev/null || true
sleep 1

nohup "$LLAMA_BIN" \
  -m "$MODEL" \
  --host "$HOST" \
  --port "$PORT" \
  -dev "$DEV" \
  -ngl "$NGL" \
  -c "$CTX" \
  -t "$THREADS" \
  >"$LOG" 2>&1 &

echo "llama-server PID=$! port=$PORT log=$LOG"
echo "Probe: curl -s http://127.0.0.1:${PORT}/v1/models"
