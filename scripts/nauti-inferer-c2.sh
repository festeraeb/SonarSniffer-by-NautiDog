#!/usr/bin/env bash
# NautiInferer coordinator on cesarops2 (home base). Forge edge :9100 for fleet sync.
#
# Usage:
#   bash scripts/nauti-inferer-c2.sh start
#   bash scripts/nauti-inferer-c2.sh stop
#   bash scripts/nauti-inferer-c2.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] && source "$REPO/scripts/cesarops2-isolated.env"
[[ -f "${HOME}/.cache/cesarops/cesarops2-isolated" ]] && source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true
[[ -d "$REPO/nauti-inferer" ]] || REPO="/codebase/repos/wreckhunter2000-1"
BIN="${NAUTI_BIN:-$REPO/target/release/nauti-inferer}"
[[ -x "$BIN" ]] || BIN="$(command -v nauti-inferer 2>/dev/null || true)"
PORT="${NAUTI_LISTEN_PORT:-8099}"
PIDFILE="${PIDFILE:-/tmp/nauti-inferer-c2.pid}"
LOG="${LOG:-/tmp/nauti-inferer-c2.log}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
DB="${NAUTI_DB_URL:-sqlite:/data/cesarops/nauti-inferer/nauti.db}"

log() { echo "[nauti-c2] $*"; }

cmd_start() {
  if [[ ! -x "$BIN" ]]; then
    log "ERROR: nauti-inferer binary missing ($BIN)"
    log "  cargo build --release -p nauti_inferer -C $REPO"
    exit 1
  fi
  mkdir -p "$(dirname "${DB#sqlite:}")"
  pkill -f "nauti-inferer.*${PORT}" 2>/dev/null || true
  fuser -k "${PORT}/tcp" 2>/dev/null || true
  sleep 1
  setsid env \
    NAUTI_MODE=coordinator \
    NAUTI_LISTEN_PORT="$PORT" \
    FORGE_URL="$FORGE_URL" \
    NAUTI_DB_URL="$DB" \
    "$BIN" </dev/null >>"$LOG" 2>&1 &
  echo $! >"$PIDFILE"
  log "coordinator pid=$(cat "$PIDFILE") :$PORT FORGE_URL=$FORGE_URL"
  for i in 1 2 3 4 5; do
    curl -sf --max-time 2 "http://127.0.0.1:${PORT}/health" >/dev/null && {
      log "OK http://127.0.0.1:${PORT}/health"
      return 0
    }
    sleep 2
  done
  log "WARN: health not ready — tail $LOG"
  tail -20 "$LOG" 2>/dev/null || true
}

cmd_stop() {
  [[ -f "$PIDFILE" ]] && kill "$(cat "$PIDFILE")" 2>/dev/null || true
  rm -f "$PIDFILE"
  pkill -f "nauti-inferer" 2>/dev/null || true
  log "stopped"
}

cmd_status() {
  if [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
    log "running pid=$(cat "$PIDFILE")"
  else
    log "not running"
  fi
  curl -sf --max-time 3 "http://127.0.0.1:${PORT}/health" && echo "nauti health OK" || echo "nauti health DOWN"
  if curl -sf --max-time 3 "${FORGE_URL}/health" >/dev/null; then
    echo "forge health OK ($FORGE_URL)"
  elif [[ "${CESAROPS2_ISOLATED:-0}" == "1" ]]; then
    echo "forge health DOWN ($FORGE_URL) — isolated mode: start scripts/cesarops2-forge-edge.sh"
  else
    echo "forge health DOWN ($FORGE_URL) — optional T440 backup: FORGE_URL=http://10.0.0.61:9100"
  fi
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
