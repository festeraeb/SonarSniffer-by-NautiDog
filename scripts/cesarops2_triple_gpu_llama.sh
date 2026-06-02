#!/usr/bin/env bash
# cesarops2 triple stack (llama-server):
#   RTX 2060 (CUDA1) — Qwen3.6 MoE turbo     :5200
#   P106-100 (CUDA0) — Qwen2.5-class 7B      :5201
#   GTX 1070 (CUDA2) — Phi-3-mini            :5202
#
# llama.cpp CUDA order (NOT nvidia-smi index): CUDA0=2060, CUDA1=1070, CUDA2=P106
#
# Usage:
#   bash scripts/cesarops2_triple_gpu_llama.sh start
#   bash scripts/cesarops2_triple_gpu_llama.sh stop
#   bash scripts/cesarops2_triple_gpu_llama.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
# GGUF live on NFS; do not use ~/cesarops-data for these weights
MODELS="${MODELS_NFS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
[[ -x "$LLAMA" ]] || LLAMA=/home/cesarops/bin/llama-server
PID_DIR="${PID_DIR:-/tmp/cesarops2-triple-gpu}"

# --- models ---
MODEL_MOE_MXFP4="${MODEL_MOE_MXFP4:-$MODELS/Qwen3.6-35B-A3B-MXFP4_MOE.gguf}"
MODEL_MOE_Q4="${MODEL_MOE_Q4:-$MODELS/Qwen3.6-35B-A3B-Q4_K_M.gguf}"
MODEL_MOE="${MODEL_MOE:-$MODEL_MOE_MXFP4}"
MOE_USE_Q4="${MOE_USE_Q4:-0}"
# Closest on-disk Qwen2.5-7B-class (4.4G fits P106); override MODEL_7B for a true Qwen2.5-7B GGUF
MODEL_7B="${MODEL_7B:-$MODELS/DeepSeek-R1-Distill-Qwen-7B-Uncensored.Q4_K_M.gguf}"
MODEL_PHI="${MODEL_PHI:-$MODELS/Phi-3-mini-4k-instruct-Q4_K_M.gguf}"

PORT_MOE="${PORT_MOE:-5200}"
PORT_7B="${PORT_7B:-5201}"
PORT_PHI="${PORT_PHI:-5202}"

DEV_MOE="${DEV_MOE:-CUDA0}"   # RTX 2060 SUPER
DEV_7B="${DEV_7B:-CUDA2}"     # P106-100
DEV_PHI="${DEV_PHI:-CUDA1}"   # GTX 1070

CTX_MOE="${CTX_MOE:-3072}"
CTX_7B="${CTX_7B:-4096}"
CTX_PHI="${CTX_PHI:-4096}"
NGL_7B="${NGL_7B:-99}"
NGL_PHI="${NGL_PHI:-99}"

log() { echo "[triple-gpu] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  pkill -f "llama-server.*-port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

stop_all() {
  for p in "$PORT_MOE" "$PORT_7B" "$PORT_PHI" 5200 5571; do stop_port "$p"; done
  pkill -f 'llama-server.*CUDA' 2>/dev/null || true
  sleep 2
}

probe() { curl -sf --max-time 5 "http://127.0.0.1:$1/v1/models" >/dev/null; }

start_one() {
  local name=$1 model=$2 port=$3 dev=$4 ctx=$5
  shift 5
  local -a extra=("$@")
  [[ -f "$model" ]] || { log "ERROR: missing $model"; return 1; }
  stop_port "$port"
  local logf="$PID_DIR/llama-${port}.log"
  mkdir -p "$PID_DIR"
  log "$name port=$port dev=$dev model=$(basename "$model")"
  setsid "$LLAMA" -m "$model" --host 0.0.0.0 --port "$port" \
    -dev "$dev" "${extra[@]}" -c "$ctx" -t 4 -np 1 \
    </dev/null >>"$logf" 2>&1 &
  echo $! >"$PID_DIR/llama-${port}.pid"
}

cmd_start() {
  mkdir -p "$PID_DIR"
  stop_all
  # Phi on 1070 first (fast), then 7B P106, then MoE 2060 (slowest load)
  start_one phi "$MODEL_PHI" "$PORT_PHI" "$DEV_PHI" "$CTX_PHI" \
    -ngl "$NGL_PHI" -fa off -ctk f16 -ctv f16 -ub 384
  sleep 2
  start_one qwen7b "$MODEL_7B" "$PORT_7B" "$DEV_7B" "$CTX_7B" \
    -ngl "$NGL_7B" -fa off -ctk f16 -ctv f16 -ub 384
  sleep 2
  if [[ "$MOE_USE_Q4" == "1" ]]; then
    start_one qwen-moe "$MODEL_MOE_Q4" "$PORT_MOE" "$DEV_MOE" "$CTX_MOE" \
      -sm layer --fit on -fa auto -ctk q8_0 -ctv q8_0 -ub 384 --no-mmap --fit-target "${MOE_FIT_TARGET:-7680}"
  else
  # 21G MXFP4 MoE does not fit 8GB VRAM alone — layer fit spills to CPU RAM
    start_one qwen-moe "$MODEL_MOE_MXFP4" "$PORT_MOE" "$DEV_MOE" "$CTX_MOE" \
      -sm layer --fit on -fa auto -ctk q8_0 -ctv q8_0 -ub 384 --no-mmap --fit-target "${MOE_FIT_TARGET:-7680}"
  fi
  log "waiting up to 300s for APIs…"
  local ok_m=0 ok_7=0 ok_p=0
  for _ in $(seq 1 60); do
    probe "$PORT_PHI" && ok_p=1
    probe "$PORT_7B" && ok_7=1
    probe "$PORT_MOE" && ok_m=1
    [[ "$ok_m" == 1 && "$ok_7" == 1 && "$ok_p" == 1 ]] && break
    sleep 5
  done
  cmd_status
  [[ "$ok_m" == 1 && "$ok_7" == 1 && "$ok_p" == 1 ]] || {
    log "WARN: see $PID_DIR/*.log"
    tail -15 "$PID_DIR"/*.log 2>/dev/null || true
  }
}

cmd_stop() {
  stop_all
  rm -f "$PID_DIR"/*.pid 2>/dev/null || true
  log "stopped"
}

cmd_status() {
  probe "$PORT_MOE" && log "OK  qwen-moe  :$PORT_MOE ($DEV_MOE)" || log "DOWN qwen-moe  :$PORT_MOE"
  probe "$PORT_7B" && log "OK  qwen-7b   :$PORT_7B ($DEV_7B)" || log "DOWN qwen-7b   :$PORT_7B"
  probe "$PORT_PHI" && log "OK  phi-3     :$PORT_PHI ($DEV_PHI)" || log "DOWN phi-3     :$PORT_PHI"
  nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv 2>/dev/null || true
  log "  moe: http://127.0.0.1:${PORT_MOE}/v1/chat/completions"
  log "  7b:  http://127.0.0.1:${PORT_7B}/v1/chat/completions"
  log "  phi: http://127.0.0.1:${PORT_PHI}/v1/chat/completions"
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
