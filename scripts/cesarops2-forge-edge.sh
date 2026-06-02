#!/usr/bin/env bash
# Forge edge on cesarops2 — primary control plane for NautiInferer (:8099).
# T440 Forge (:9100 on .61) remains backup; switch with forge-routing-switch.sh
#
# Usage:
#   bash scripts/cesarops2-forge-edge.sh start
#   bash scripts/cesarops2-forge-edge.sh stop
#   bash scripts/cesarops2-forge-edge.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true
[[ -f "$REPO/scripts/cesarops2-forge-edge.sh" ]] || REPO="/codebase/repos/wreckhunter2000-1"
FORGE_DIR="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
PORT="${FORGE_EDGE_PORT:-9100}"
PIDFILE="${PIDFILE:-/tmp/cesarops2-forge-edge.pid}"
LOG="${LOG:-/tmp/cesarops2-forge-edge.log}"
FORGE_BIN="${FORGE_BIN:-$FORGE_DIR/target/release/cesarops-forge-v2}"
[[ -x "$FORGE_BIN" ]] || FORGE_BIN="${FORGE_BIN:-/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/target/release/cesarops-forge-v2}"
[[ -x "$FORGE_BIN" ]] || FORGE_BIN="$(command -v cesarops-forge-v2 2>/dev/null || true)"

log() { echo "[forge-edge] $*"; }

stop_port() {
  pkill -f "cesarops-forge-v2.*${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
}

cmd_start() {
  if [[ ! -x "$FORGE_BIN" ]]; then
    log "ERROR: Forge binary not found ($FORGE_BIN). Build on T440 or copy binary."
    exit 1
  fi
  bash "$REPO/scripts/forge-routing-switch.sh" edge
  stop_port
  sleep 1
  export FORGE_V2_DIR="$FORGE_DIR"
  export FORGE_ROUTING_STATE="$FORGE_DIR/routing_state.json"
  export FORGE_MODE_STATE="$FORGE_DIR/mode_state.json"
  export FORGE_CLUSTER_CONFIG="$FORGE_DIR/cluster_config.toml"
  # Forge v2 listens on :9100 (hardcoded in binary).
  setsid env FORGE_V2_DIR="$FORGE_V2_DIR" \
    "$FORGE_BIN" \
    </dev/null >>"$LOG" 2>&1 &
  echo $! >"$PIDFILE"
  log "started pid=$(cat "$PIDFILE") port=$PORT log=$LOG"
  for i in 1 2 3 4 5 6 7 8 9 10; do
    curl -sf --max-time 2 "http://127.0.0.1:${PORT}/health" >/dev/null && {
      log "OK http://127.0.0.1:${PORT}"
      return 0
    }
    sleep 2
  done
  log "WARN: health check failed — tail $LOG"
  tail -15 "$LOG" 2>/dev/null || true
}

cmd_stop() {
  if [[ -f "$PIDFILE" ]]; then
    kill "$(cat "$PIDFILE")" 2>/dev/null || true
    rm -f "$PIDFILE"
  fi
  stop_port
  log "stopped"
}

cmd_status() {
  if [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
    log "running pid=$(cat "$PIDFILE") port=$PORT"
  else
    log "not running"
  fi
  curl -sf --max-time 3 "http://127.0.0.1:${PORT}/health" && echo "health OK" || echo "health DOWN"
  FORGE_URL="http://127.0.0.1:${PORT}" bash "$REPO/scripts/forge-routing-switch.sh" status
}

case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {start|stop|status}"
    exit 1
  ;;
esac
