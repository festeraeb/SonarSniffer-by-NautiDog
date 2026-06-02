#!/usr/bin/env bash
# P100 #1 = Vulkan reviewer/aux (port :5002). P100 #0 stays on CUDA Gemma :5001 (coding).
set -euo pipefail

LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
PORT="${PORT:-5002}"
# Fits P100 #1 (16GB Vulkan1) while #0 stays on CUDA Gemma :5001. Override MODEL for larger quants.
MODEL="${MODEL:-/codebase/models/Qwen2.5-Coder-14B-Instruct-abliterated-Q4_K_M.gguf}"
LOG="${LOG:-/tmp/p100-vulkan-5002.log}"
CTX="${CTX:-8192}"
# With --fit on, do not pass -ngl (llama aborts if both are set).
NGL="${NGL:-}"

free_port() {
  pkill -9 -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  pkill -9 -f "llama-server.*-port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

[[ -f "$MODEL" ]] || { echo "[p100-vulkan] missing model: $MODEL"; exit 1; }
[[ -x "$LLAMA" ]] || { echo "[p100-vulkan] missing llama-server: $LLAMA"; exit 1; }

free_port
: >"$LOG"

echo "[p100-vulkan] starting Vulkan1 :${PORT} model=$(basename "$MODEL")"
# Vulkan1 = second P100; CUDA0 on GPU0 remains Gemma coder
llama_args=(
  -m "$MODEL"
  --host 0.0.0.0
  --port "$PORT"
  -dev Vulkan1
  -sm layer
  -c "$CTX"
  -t 4
  --fit on
)
if [[ -n "$NGL" ]]; then
  llama_args+=(-ngl "$NGL")
fi
setsid "$LLAMA" "${llama_args[@]}" >>"$LOG" 2>&1 &

echo $! >"/tmp/p100-vulkan-${PORT}.pid"
echo "[p100-vulkan] pid=$(cat /tmp/p100-vulkan-${PORT}.pid) log=$LOG"

for i in $(seq 1 120); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    echo "[p100-vulkan] ready :${PORT}"
    curl -sf "http://127.0.0.1:${PORT}/v1/models" | head -c 200
    echo
    exit 0
  fi
  sleep 2
done
echo "[p100-vulkan] WARN: not ready — tail $LOG"
tail -20 "$LOG"
exit 1
