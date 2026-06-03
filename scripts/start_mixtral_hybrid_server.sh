#!/usr/bin/env bash
# Mixtral thinker: RTX (partial layers) + CPU (remainder). Replaces Qwen/Gemma on :5200.
#
#   PORT=5200 NGL_RTX=12 bash scripts/start_mixtral_hybrid_server.sh
#   THINKER_MODE=cpu PORT=5211 bash scripts/start_mixtral_hybrid_server.sh  # CPU-only lane
set -euo pipefail

MODEL="${MODEL:-/mnt/t440/models/Mixtral-8x7B-Instruct-v0.1.Q5_K_M.gguf}"
PORT="${PORT:-5200}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
LOG="${LOG:-/tmp/llama-mixtral-${PORT}.log}"
THREADS="${THREADS:-16}"
CTX="${CTX:-8192}"

# cpu | hybrid (RTX + CPU)
THINKER_MODE="${THINKER_MODE:-hybrid}"
# Mixtral Q5 ~31GB: only a few layers fit on 8GB RTX; rest stays on CPU RAM.
NGL_RTX="${NGL_RTX:-4}"
GPU_DEV="${GPU_DEV:-CUDA0}"

log() { echo "[mixtral-thinker] $*"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 2
}

stop_port
mkdir -p "$(dirname "$LOG")"

if [[ ! -f "$MODEL" ]]; then
  log "ERROR: missing model $MODEL"
  exit 1
fi

if [[ "$THINKER_MODE" == "cpu" ]]; then
  log "CPU-only thinker :$PORT ctx=$CTX threads=$THREADS"
  exec "$LLAMA" \
    --model "$MODEL" \
    --port "$PORT" --host 127.0.0.1 \
    -dev none \
    --threads "$THREADS" \
    -np 1 -c "$CTX" \
    --cache-ram -1 --no-warmup \
    >>"$LOG" 2>&1
fi

# hybrid: offload NGL_RTX layers to RTX; remainder in host RAM (not -dev none full offload)
log "hybrid thinker :$PORT dev=$GPU_DEV ngl=$NGL_RTX ctx=$CTX" | tee -a "$LOG"
exec "$LLAMA" \
  --model "$MODEL" \
  --host 0.0.0.0 --port "$PORT" \
  -dev "$GPU_DEV" \
  -ngl "$NGL_RTX" \
  -fa auto -ctk q8_0 -ctv q8_0 \
  -ub 384 -c "$CTX" \
  -t "$THREADS" -np 1 \
  --no-mmap \
  --reasoning off \
  >>"$LOG" 2>&1
