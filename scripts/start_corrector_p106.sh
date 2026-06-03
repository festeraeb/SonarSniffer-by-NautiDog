#!/usr/bin/env bash
# Corrector on cesarops2 P106 (:5201). 1070 left free for future PAMP — not wired in dual-lane flow.
#
#   bash scripts/start_corrector_p106.sh
set -euo pipefail

MODEL="${MODEL:-/mnt/t440/models/Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf}"
[[ -f "$MODEL" ]] || MODEL="/data/codebase/repos/wreckhunter2000-1/models/Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf"

PORT="${PORT:-5201}"
GPU_DEV="${GPU_DEV:-CUDA2}"
NGL="${NGL:-28}"
CTX="${CTX:-4096}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
LOG="${LOG:-/tmp/llama-corrector-p106-${PORT}.log}"

log() { echo "[corrector-p106] $*"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

[[ -f "$MODEL" ]] || { log "WARN: missing 7B GGUF — corrector optional"; exit 0; }

stop_port
log "starting $(basename "$MODEL") :$PORT dev=$GPU_DEV ngl=$NGL"

setsid "$LLAMA" \
  --model "$MODEL" \
  --host 0.0.0.0 --port "$PORT" \
  -dev "$GPU_DEV" \
  -ngl "$NGL" \
  -fa auto -ctk q8_0 -ctv q8_0 \
  -ub 384 -c "$CTX" \
  -t 4 -np 1 \
  --reasoning off --timeout 600 \
  >>"$LOG" 2>&1 &

for _ in $(seq 1 36); do
  curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null && {
    log "ready http://127.0.0.1:${PORT}"
    exit 0
  }
  sleep 5
done
log "failed — tail $LOG"
exit 1
