#!/usr/bin/env bash
# T440 side of dual-lane layout (run ON T440):
#   :5001 P100 — Gemma 4 MoE (Lane A coder)
#   :5002 P100 — Qwen2.5-Coder-14B (Lane B coder)
#   :5010 CPU — Qwen3.6 MoE (Lane B thinker + polisher)
#
#   bash scripts/t440_dual_lane_layout.sh start|stop|status
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"

log() { echo "[t440-dual-lane] $*"; }

cmd_start() {
  log "Gemma :5001 + Qwen14 :5002 on dual P100"
  bash "$REPO/scripts/p100_gemma_r1_dual.sh" start
  log "Qwen3.6 MoE CPU :5010 (Lane B think/polish)"
  bash "$REPO/scripts/start_qwen36_moe_cpu_laneb.sh"
  cmd_status
}

cmd_stop() {
  bash "$REPO/scripts/p100_gemma_r1_dual.sh" free
  PORT=5010 pkill -f 'llama-server.*--port 5010' 2>/dev/null || true
  log "stopped T440 dual-lane slots"
}

cmd_status() {
  bash "$REPO/scripts/p100_gemma_r1_dual.sh" status
  if curl -sf --max-time 3 http://127.0.0.1:5010/v1/models >/dev/null; then
    echo "up :5010 (Qwen3.6 CPU lane B think/polish)"
  else
    echo "down :5010"
  fi
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
