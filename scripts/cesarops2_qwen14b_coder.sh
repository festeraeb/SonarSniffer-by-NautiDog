#!/usr/bin/env bash
# cesarops2: Qwen2.5-Coder-14B-Instruct on RTX 2060 (:5200).
# Replaces Qwen3.6 MoE on :5200 when you need stable coding (not MoE).
#
# Previous jobs "quit" because:
#   - ctx was 2048 but Forge/dispatch sends 3k–5k tokens → llama returns error and clients stall
#   - Qwen3.6 MoE on :5200 was killed by interrupt / port fights
#
# Usage (on cesarops2):
#   bash scripts/cesarops2_qwen14b_coder.sh start
#   bash scripts/cesarops2_qwen14b_coder.sh status
#   bash scripts/cesarops2_qwen14b_coder.sh stop
#   bash scripts/cesarops2_qwen14b_coder.sh restart
#
# From T440:
#   ssh cesarops@10.0.0.201 'bash /mnt/t440/codebase/repos/wreckhunter2000-1/scripts/cesarops2_qwen14b_coder.sh start'
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2_qwen14b_coder.sh" ]] || REPO="/codebase/repos/wreckhunter2000-1"
MODELS="${MODELS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
PID_DIR="${PID_DIR:-/tmp/cesarops2-qwen14b}"

MODEL="${MODEL:-$MODELS/qwen2.5-coder-14b-instruct-q4_k_m.gguf}"
PORT="${PORT:-5200}"
DEV="${DEV:-CUDA0}"          # llama.cpp CUDA0 = RTX 2060 on cesarops2
CTX="${CTX:-6144}"           # 8192 OOMs on 8GB 2060 with KV; 2048 broke large jobs
NGL="${NGL:-34}"             # 99 → cudaMalloc OOM; ~34 layers fits 2060 + CPU spill
PARALLEL="${PARALLEL:-1}"    # keep 1 slot — parallel slots exhaust KV on 8GB
USE_FIT="${USE_FIT:-0}"      # set 1 only if NGL unset (do not combine -ngl 99 + --fit)

log() { echo "[qwen14b-2060] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

stop_zaya() {
  if pgrep -f 'serve_zaya_api.py' >/dev/null 2>&1; then
    log "stopping ZAYA :8010 (frees 2060 VRAM)…"
    pkill -f 'serve_zaya_api.py' 2>/dev/null || true
    sleep 2
  fi
}

cmd_start() {
  [[ -f "$MODEL" ]] || { log "ERROR: missing $MODEL"; exit 1; }
  [[ -x "$LLAMA" ]] || { log "ERROR: missing $LLAMA"; exit 1; }
  mkdir -p "$PID_DIR"
  stop_zaya
  stop_port "$PORT"
  sleep 2

  local logf="$PID_DIR/llama-${PORT}.log"
  log "starting $(basename "$MODEL") on :$PORT dev=$DEV ctx=$CTX ngl=$NGL np=$PARALLEL"
  local -a extra=(-sm layer -fa auto -ctk q8_0 -ctv q8_0 -ub 256)
  if [[ "$USE_FIT" == "1" ]]; then
  extra+=(--fit on --fit-target "${FIT_TARGET:-7168}")
  else
  extra+=(-ngl "$NGL")
  fi
  setsid "$LLAMA" \
    -m "$MODEL" \
    --host 0.0.0.0 \
    --port "$PORT" \
    -dev "$DEV" \
    "${extra[@]}" \
    -c "$CTX" \
    -t 4 \
    -np "$PARALLEL" \
    --timeout 600 \
    --reasoning off \
    </dev/null >>"$logf" 2>&1 &
  echo $! >"$PID_DIR/llama-${PORT}.pid"
  log "pid=$(cat "$PID_DIR/llama-${PORT}.pid") log=$logf"

  local ok=0
  for i in $(seq 1 48); do
    if curl -sf --max-time 5 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
      ok=1
      break
    fi
    sleep 5
  done
  if [[ "$ok" == 1 ]]; then
    log "READY http://10.0.0.201:${PORT}/v1/chat/completions"
    curl -sf "http://127.0.0.1:${PORT}/v1/models" | head -c 200 || true
    echo
  else
    log "WARN: not ready — tail $logf"
    tail -20 "$logf" 2>/dev/null || true
    exit 1
  fi
  nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
}

cmd_stop() {
  stop_zaya
  stop_port "$PORT"
  rm -f "$PID_DIR"/*.pid 2>/dev/null || true
  log "stopped :$PORT"
}

cmd_status() {
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    log "UP :$PORT"
    curl -sf "http://127.0.0.1:${PORT}/v1/models" | head -c 240
    echo
  else
    log "DOWN :$PORT"
  fi
  pgrep -af "llama-server.*${PORT}" || true
  nvidia-smi --query-gpu=index,name,memory.used --format=csv 2>/dev/null || true
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  restart) cmd_stop; sleep 2; cmd_start ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {start|stop|restart|status}"
    exit 1
    ;;
esac
