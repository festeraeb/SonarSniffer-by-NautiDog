#!/usr/bin/env bash
# DEPRECATED: use scripts/start_qwen14_coder_p100.sh (:5002) + leave :5203 idle.
# Qwen2.5-Coder-14B on GTX 1070 (:5203) — partial GPU + CPU offload.
#
#   bash scripts/start_qwen_coder_14b_1070.sh
#   NGL_1070=12 CTX=4096 bash scripts/start_qwen_coder_14b_1070.sh
set -euo pipefail

MODEL="${MODEL:-/mnt/t440/models/Qwen2.5-Coder-14B-Instruct-abliterated-Q4_K_M.gguf}"
[[ -f "$MODEL" ]] || MODEL="/mnt/t440/models/qwen2.5-coder-14b-instruct-q4_k_m.gguf"

PORT="${PORT:-5203}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
LOG="${LOG:-/tmp/llama-qwen14-1070-${PORT}.log}"
# CUDA0=RTX (Mixtral), CUDA1=1070, CUDA2=P106 — keep 14B off RTX while Mixtral runs
GPU_DEV="${GPU_DEV:-CUDA1}"
NGL_1070="${NGL_1070:-10}"
CTX="${CTX:-4096}"
THREADS="${THREADS:-10}"

log() { echo "[qwen14-1070] $*"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

if [[ ! -f "$MODEL" ]]; then
  log "ERROR: missing 14B GGUF under /mnt/t440/models"
  exit 1
fi

stop_port
mkdir -p "$(dirname "$LOG")"
log "starting $(basename "$MODEL") :$PORT dev=$GPU_DEV ngl=$NGL_1070 ctx=$CTX threads=$THREADS"

setsid "$LLAMA" \
  --model "$MODEL" \
  --host 0.0.0.0 --port "$PORT" \
  -dev "$GPU_DEV" \
  -ngl "$NGL_1070" \
  -fa auto -ctk q8_0 -ctv q8_0 \
  -ub 384 -c "$CTX" \
  -t "$THREADS" -np 1 \
  --no-mmap \
  --reasoning off --timeout 600 \
  >>"$LOG" 2>&1 &

for _ in $(seq 1 48); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    log "ready http://127.0.0.1:${PORT}"
    exit 0
  fi
  sleep 5
done

log "failed — tail $LOG"
tail -30 "$LOG" >&2 || true
exit 1
