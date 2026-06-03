#!/usr/bin/env bash
# cesarops2 side of dual-lane layout:
#   :5200 RTX  — Mixtral (Lane A thinker/polish)
#   :5201 P106 — Corrector 7B (optional; not in 3-step pipeline)
#   :5203     — STOPPED (1070 free for future PAMP)
#
# T440 (run separately): bash scripts/t440_dual_lane_layout.sh start
#
#   bash scripts/cesarops2_dual_lane_layout.sh start|stop|status
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
T440="${T440_LAN:-10.0.0.61}"

log() { echo "[c2-dual-lane] $*"; }

cmd_start() {
  log "Mixtral thinker :5200 (CUDA0 RTX)"
  GPU_DEV=CUDA0 NGL_RTX="${NGL_RTX:-4}" CTX_RTX="${CTX_RTX:-8192}" \
    bash "$REPO/scripts/cesarops2_mixtral_thinker_layout.sh" start

  log "leave :5203 down (1070 reserved — no PAMP in this flow)"
  pkill -f 'llama-server.*--port 5203' 2>/dev/null || true

  log "corrector :5201 on P106 (optional)"
  bash "$REPO/scripts/start_corrector_p106.sh" || log "WARN corrector not started"

  log "probe T440 slots (start there if down)"
  for p in 5001 5002 5010; do
    if curl -sf --max-time 5 "http://${T440}:${p}/v1/models" >/dev/null; then
      log "OK T440 :$p"
    else
      log "WARN T440 :$p down — on T440: bash scripts/t440_dual_lane_layout.sh start"
    fi
  done
  bash "$REPO/scripts/forge_apply_dual_lane_routing.sh" 2>/dev/null || true
  cmd_status
}

cmd_stop() {
  bash "$REPO/scripts/cesarops2_mixtral_thinker_layout.sh" stop
  pkill -f 'llama-server.*--port 5201' 2>/dev/null || true
  pkill -f 'llama-server.*--port 5203' 2>/dev/null || true
  log "stopped c2 dual-lane (Mixtral + P106 corrector)"
}

cmd_status() {
  bash "$REPO/scripts/cesarops2_mixtral_thinker_layout.sh" status
  if curl -sf --max-time 3 http://127.0.0.1:5201/v1/models >/dev/null; then
    echo "up :5201 P106 corrector"
  else
    echo "down :5201"
  fi
  echo "down-by-design :5203 (1070 idle)"
  echo "=== T440 (from c2) ==="
  for p in 5001 5002 5010; do
    curl -sf --max-time 3 "http://${T440}:${p}/v1/models" >/dev/null && echo "up :$p" || echo "down :$p"
  done
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
