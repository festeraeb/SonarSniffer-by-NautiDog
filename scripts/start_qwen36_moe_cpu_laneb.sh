#!/usr/bin/env bash
# Lane B thinker + polisher: Qwen3.6 MoE on T440 CPU (:5010). Frees P100-1 for Qwen14 coder (:5002).
#
#   bash scripts/start_qwen36_moe_cpu_laneb.sh
#   THREADS=24 CTX=8192 bash scripts/start_qwen36_moe_cpu_laneb.sh
set -euo pipefail

MODEL="${MODEL:-}"
resolve() {
  for cand in \
    /mnt/t440/models/Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf \
    /mnt/t440/models/Qwen3.6-35B-A3B-Q4_K_M.gguf \
    /mnt/t440/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \
    /codebase/models/Qwen3.6-35B-A3B-MXFP4_MOE.gguf \
    /codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf; do
    [[ -f "$cand" ]] && { echo "$cand"; return 0; }
  done
  return 1
}
MODEL="${MODEL:-$(resolve || true)}"

PORT="${PORT:-5010}"
CTX="${CTX:-8192}"
THREADS="${THREADS:-$(nproc 2>/dev/null || echo 16)}"
LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
LOG="${LOG:-/data/cesarops/logs/qwen36-cpu-laneb-${PORT}.log}"

log() { echo "[qwen36-cpu-laneb] $*"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

[[ -n "$MODEL" && -f "$MODEL" ]] || { log "ERROR: no Qwen3.6 GGUF found"; exit 1; }

mkdir -p "$(dirname "$LOG")"
stop_port
log "starting $(basename "$MODEL") CPU :$PORT threads=$THREADS ctx=$CTX"

NUMA_ARGS=()
if "$LLAMA" --help 2>&1 | grep -q numa; then
  NUMA_ARGS=(--numa distribute)
fi

setsid "$LLAMA" \
  -m "$MODEL" \
  --host 0.0.0.0 --port "$PORT" \
  -ngl 0 \
  "${NUMA_ARGS[@]}" \
  -c "$CTX" \
  -t "$THREADS" \
  -np 1 \
  --reasoning on \
  >>"$LOG" 2>&1 &

for _ in $(seq 1 90); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    log "ready http://127.0.0.1:${PORT} (CPU — plan/polish may be slow)"
    exit 0
  fi
  sleep 5
done
log "failed — tail $LOG"
tail -30 "$LOG" >&2 || true
exit 1
