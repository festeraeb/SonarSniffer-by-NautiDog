#!/usr/bin/env bash
# T440: Qwen3-Coder-Next IQ3 on CPU only — polisher lane (:5010).
# Run AFTER coders; use when output still fails review.
#
#   bash scripts/t440_polisher_coder_next_cpu.sh start
#   bash scripts/t440_polisher_coder_next_cpu.sh stop
#   bash scripts/t440_polisher_coder_next_cpu.sh status
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
LLAMA="${LLAMA:-/home/cesarops/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server"

PORT="${PORT:-5010}"
CTX="${CTX:-4096}"
THREADS="${THREADS:-16}"
PID_DIR="${PID_DIR:-/tmp/t440-polisher-coder-next}"
LOG="${LOG:-/data/cesarops/logs/t440-polisher-coder-next.log}"

MODEL="${MODEL:-}"
resolve_model() {
  for cand in \
    /data/cesarops/models/Qwen_Qwen3-Coder-Next-IQ3_M.gguf \
    /mnt/t440/data/cesarops/models/Qwen_Qwen3-Coder-Next-IQ3_M.gguf \
    /codebase/models/Qwen_Qwen3-Coder-Next-IQ3_M.gguf; do
    [[ -f "$cand" ]] && { echo "$cand"; return 0; }
  done
  return 1
}
MODEL="${MODEL:-$(resolve_model || true)}"

log() { echo "[polisher-cpu] $*" | tee -a "$LOG"; }

stop_port() {
  pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
}

cmd_start() {
  [[ -n "$MODEL" && -f "$MODEL" ]] || {
    log "ERROR: missing Qwen3-Coder-Next IQ3 — run scripts/download_qwen3_coder_next_80b_iq3.sh"
    exit 1
  }
  mkdir -p "$PID_DIR" "$(dirname "$LOG")"
  stop_port
  sleep 2
  log "starting CPU-only polisher $(basename "$MODEL") on :$PORT threads=$THREADS"
  # -ngl 0 = CPU inference; keep P100 VRAM free
  setsid "$LLAMA" \
    -m "$MODEL" \
    --host 0.0.0.0 \
    --port "$PORT" \
    -ngl 0 \
    -c "$CTX" \
    -t "$THREADS" \
    -np 1 \
    --reasoning off \
    --timeout 900 \
    </dev/null >>"$PID_DIR/llama-${PORT}.log" 2>&1 &
  echo $! >"$PID_DIR/llama-${PORT}.pid"
  for i in $(seq 1 120); do
    if curl -sf --max-time 5 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
      log "READY http://127.0.0.1:${PORT}/v1/chat/completions"
      exit 0
    fi
    sleep 5
  done
  log "WARN: slow CPU load — tail $PID_DIR/llama-${PORT}.log"
  tail -20 "$PID_DIR/llama-${PORT}.log" 2>/dev/null || true
}

cmd_stop() { stop_port; log "stopped :$PORT"; }
cmd_status() {
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    curl -sf "http://127.0.0.1:${PORT}/v1/models" | head -c 200
    echo
  else
    log "DOWN :$PORT"
  fi
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
