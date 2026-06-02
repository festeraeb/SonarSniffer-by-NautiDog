#!/usr/bin/env bash
# cesarops2 fleet roles per docs/FLEET_GPU_LAYOUT.md
#   RTX 2060  :5200 — Gemma-4-E4B (dispatcher / thinker)
#   GTX 1070  :5202 — Qwen2.5-Coder-7B (fast scripting)
#   P106      :5201 — stopped (leave VRAM for watchdog optional Phi)
#
#   bash scripts/cesarops2_fleet_roles.sh start
#   bash scripts/cesarops2_fleet_roles.sh stop
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2_fleet_roles.sh" ]] || REPO="/data/codebase/repos/wreckhunter2000-1"
MODELS="${MODELS:-/mnt/t440/models}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
PID_DIR="${PID_DIR:-/tmp/cesarops2-fleet-roles}"

PORT_RTX="${PORT_RTX:-5200}"
PORT_1070="${PORT_1070:-5202}"
MODEL_RTX="${MODEL_RTX:-$MODELS/gemma-4-E4B-it-Q4_K_M.gguf}"
MODEL_1070="${MODEL_1070:-$MODELS/Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf}"

log() { echo "[fleet-roles] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

start_one() {
  local model=$1 port=$2 dev=$3 ngl=$4 ctx=$5
  stop_port "$port"
  mkdir -p "$PID_DIR"
  setsid "$LLAMA" -m "$model" --host 0.0.0.0 --port "$port" -dev "$dev" \
    -ngl "$ngl" -fa auto -ctk q8_0 -ctv q8_0 -ub 384 -c "$ctx" -t 4 -np 1 \
    --reasoning off --timeout 600 </dev/null >>"$PID_DIR/llama-${port}.log" 2>&1 &
  log "started $(basename "$model") :$port dev=$dev ngl=$ngl"
}

wait_port() {
  local port=$1
  for _ in $(seq 1 48); do
    curl -sf --max-time 3 "http://127.0.0.1:${port}/v1/models" >/dev/null && return 0
    sleep 5
  done
  return 1
}

cmd_start() {
  if [[ "${FLEET_UNIFIED:-0}" != "1" && ! -f "${HOME}/.cache/cesarops/fleet-unified" ]]; then
    touch "${HOME}/.cache/cesarops/cesarops2-isolated" 2>/dev/null || true
  fi
  export LOCAL_CARD_AUTO_RECOVER=0
  for p in 5200 5201 5202 5210 5212 5571; do stop_port "$p"; done
  sleep 2
  if [[ "${FLEET_UNIFIED:-0}" == "1" || -f "${HOME}/.cache/cesarops/fleet-unified" ]]; then
    # ZAYA thinker on :5203 is started by cesarops2_unified_layout.sh
    start_one "$MODEL_RTX" "$PORT_RTX" CUDA1 99 8192
    wait_port "$PORT_RTX" && log "OK :$PORT_RTX" || log "WARN :$PORT_RTX"
  else
    # 1070 first (7B), then RTX E4B
    start_one "$MODEL_1070" "$PORT_1070" CUDA1 99 4096
    sleep 2
    start_one "$MODEL_RTX" "$PORT_RTX" CUDA0 99 8192
    wait_port "$PORT_1070" && log "OK :$PORT_1070" || log "WARN :$PORT_1070"
    wait_port "$PORT_RTX" && log "OK :$PORT_RTX" || log "WARN :$PORT_RTX"
  fi
  nvidia-smi --query-gpu=index,memory.used --format=csv 2>/dev/null || true
}

cmd_stop() {
  for p in 5200 5201 5202; do stop_port "$p"; done
  log "stopped c2 fleet role ports"
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status)
    for p in "$PORT_RTX" "$PORT_1070"; do
      curl -sf --max-time 2 "http://127.0.0.1:${p}/v1/models" | head -c 120 && echo " :$p" || echo "DOWN :$p"
    done
    ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
