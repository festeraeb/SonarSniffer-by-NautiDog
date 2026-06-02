#!/usr/bin/env bash
# Small llama on GTX 1070 :5599 — wake fleet back to normal mode.
set -euo pipefail

LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
MODEL="${INTAKE_MODEL:-/data/cesarops/local_models/Phi-3-mini-4k-instruct-q4.gguf}"
PORT="${INTAKE_PORT:-5599}"
GPU="${INTAKE_GPU:-2}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"

free_port() {
  pkill -f "llama-server.*port ${PORT}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 1
}

start() {
  free_port
  if [[ ! -f "$MODEL" ]]; then
    echo "[intake] model missing: $MODEL"
    exit 1
  fi
  CUDA_VISIBLE_DEVICES="$GPU" setsid "$LLAMA" \
    -m "$MODEL" --host 0.0.0.0 --port "$PORT" \
    -dev CUDA0 -ngl 99 -c 4096 -t 2 \
    >>/tmp/intake-sentinel.log 2>&1 &
  echo "[intake] started :${PORT} PID=$!"
}

case "${1:-start}" in
  start) start ;;
  stop) free_port ;;
  wake)
    curl -sf -X POST "http://127.0.0.1:${PORT}/v1/chat/completions" \
      -H 'Content-Type: application/json' \
      -d '{"messages":[{"role":"user","content":"wake"}],"max_tokens":16}' >/dev/null 2>&1 || true
    bash "${REPO}/scripts/cesarops-fleet-mode.sh" wake
    ;;
  *)
    echo "Usage: $0 {start|stop|wake}"
    exit 1
    ;;
esac
