#!/usr/bin/env bash
# Unified thinker swap: RTX :5200 = Mixtral (hybrid), optional CPU :5211 for A/B.
# Stops Qwen/Gemma draft on :5200 and ZAYA on :5203 (Mixtral is the thinker now).
#
#   bash scripts/cesarops2_mixtral_thinker_layout.sh start
#   bash scripts/cesarops2_mixtral_thinker_layout.sh status
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
PORT_RTX="${PORT_RTX:-5200}"
PORT_CPU="${PORT_CPU:-5211}"
NGL_RTX="${NGL_RTX:-4}"
START_CPU_LANE="${START_CPU_LANE:-0}"

log() { echo "[mixtral-layout] $*"; }

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}

cmd_start() {
  log "stop legacy RTX draft (Qwen/Gemma) + ZAYA :5203"
  stop_port "$PORT_RTX"
  stop_port 5203
  sleep 2

  # llama.cpp CUDA0 = RTX 2060 SUPER, CUDA1 = 1070, CUDA2 = P106 on cesarops2
  log "RTX+CPU Mixtral thinker on :$PORT_RTX (ngl=$NGL_RTX dev=${GPU_DEV:-CUDA0})"
  PORT="$PORT_RTX" NGL_RTX="$NGL_RTX" GPU_DEV="${GPU_DEV:-CUDA0}" THINKER_MODE=hybrid CTX="${CTX_RTX:-8192}" \
    setsid bash "${REPO}/scripts/start_mixtral_hybrid_server.sh" \
    >>"/tmp/mixtral-rtx-${PORT_RTX}.log" 2>&1 &
  log "pid=$! log=/tmp/mixtral-rtx-${PORT_RTX}.log /tmp/llama-mixtral-${PORT_RTX}.log"

  for _ in $(seq 1 60); do
    curl -sf --max-time 5 "http://127.0.0.1:${PORT_RTX}/health" >/dev/null && break
    sleep 10
  done

  if [[ "$START_CPU_LANE" == "1" ]]; then
    log "CPU-only reference lane :$PORT_CPU (heavy RAM — use only for bench)"
    PORT="$PORT_CPU" THINKER_MODE=cpu CTX=6144 THREADS=12 \
      nohup bash "${REPO}/scripts/start_mixtral_hybrid_server.sh" \
      >>"/tmp/mixtral-cpu-${PORT_CPU}.log" 2>&1 &
    disown || true
  else
    stop_port "$PORT_CPU"
    log "CPU lane :$PORT_CPU stopped (set START_CPU_LANE=1 to run RTX vs CPU bench)"
  fi

  bash "${REPO}/scripts/forge_apply_mixtral_thinker.sh" || true
  cmd_status
}

cmd_stop() {
  stop_port "$PORT_RTX"
  stop_port "$PORT_CPU"
  stop_port 5203
  log "stopped Mixtral thinker ports"
}

cmd_status() {
  for p in "$PORT_RTX" "$PORT_CPU"; do
    if curl -sf --max-time 3 "http://127.0.0.1:${p}/v1/models" >/dev/null; then
      id=$(curl -sf "http://127.0.0.1:${p}/v1/models" | python3 -c "import sys,json; print(json.load(sys.stdin)['data'][0]['id'])" 2>/dev/null || echo "?")
      echo "up :$p → $id"
    else
      echo "down :$p"
    fi
  done
  pgrep -af 'llama-server.*--port (5200|5211)' 2>/dev/null | head -4 || true
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
