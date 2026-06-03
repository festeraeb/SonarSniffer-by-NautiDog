#!/usr/bin/env bash
# Lane B coder: Qwen2.5-Coder-14B on T440 P100-1 (:5002). MoE thinker runs on CPU :5010.
#
#   bash scripts/start_qwen14_coder_p100.sh
#   P100_GPU=0 CTX=8192 bash scripts/start_qwen14_coder_p100.sh
set -euo pipefail

MODEL="${MODEL:-}"
[[ -n "$MODEL" && -f "$MODEL" ]] || MODEL="/mnt/t440/models/Qwen2.5-Coder-14B-Instruct-abliterated-Q4_K_M.gguf"
[[ -f "$MODEL" ]] || MODEL="/mnt/t440/models/qwen2.5-coder-14b-instruct-q4_k_m.gguf"
[[ -f "$MODEL" ]] || MODEL="/codebase/models/Qwen2.5-Coder-14B-Instruct-abliterated-Q4_K_M.gguf"

PORT="${PORT:-5002}"
P100_GPU="${P100_GPU:-0}"
CTX="${CTX:-8192}"
LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
LOG="${LOG:-/data/cesarops/logs/qwen14-p100-${PORT}.log}"

log() { echo "[qwen14-p100] $*"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

[[ -f "$MODEL" ]] || { log "ERROR: missing 14B GGUF"; exit 1; }
[[ -x "$LLAMA" ]] || { log "ERROR: llama-server not found"; exit 1; }

mkdir -p "$(dirname "$LOG")"
stop_port
log "starting $(basename "$MODEL") :$PORT P100_GPU=$P100_GPU ctx=$CTX"

CUDA_VISIBLE_DEVICES="$P100_GPU" setsid "$LLAMA" \
  -m "$MODEL" \
  --host 0.0.0.0 --port "$PORT" \
  -dev CUDA0 \
  -sm layer --fit on \
  --reasoning off \
  -c "$CTX" \
  -t 4 \
  >>"$LOG" 2>&1 &

for _ in $(seq 1 60); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    log "ready http://127.0.0.1:${PORT}"
    exit 0
  fi
  sleep 5
done
log "failed — tail $LOG"
tail -30 "$LOG" >&2 || true
exit 1
