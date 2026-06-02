#!/usr/bin/env bash
# Isolate cesarops2 from T440: local Forge/Nauti/Cake only; no P100/T440 fleet jobs.
#
# Usage:
#   bash scripts/cesarops2-isolate-from-t440.sh on
#   bash scripts/cesarops2-isolate-from-t440.sh off
#   bash scripts/cesarops2-isolate-from-t440.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-isolated.env" ]] || REPO="/codebase/repos/wreckhunter2000-1"
MARK="${HOME}/.cache/cesarops/cesarops2-isolated"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"

log() { echo "[c2-isolate] $*"; }

stop_t440_coupling() {
  # Stop Cake cluster-key mode that registers a "t440-fleet" worker on this box
  CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-$HOME/.cache/cesarops/cake-cluster.key}" \
    bash "$REPO/scripts/cake/stop-fleet-cluster.sh" 2>/dev/null || true
  pkill -f 'cake run.*t440-fleet' 2>/dev/null || true
  # Do not touch T440 over SSH; do not queue t440 fleet jobs
  log "stopped local Cake fleet coupling (T440 untouched)"
}

cmd_on() {
  mkdir -p "$(dirname "$MARK")"
  date -u +%Y-%m-%dT%H:%M:%SZ >"$MARK"
  stop_t440_coupling
  bash "$REPO/scripts/forge-routing-switch.sh" edge 2>/dev/null || true
  log "isolation ON — marker $MARK"
  log "source: $REPO/scripts/cesarops2-isolated.env"
  log "T440 (Zaya/P100) will not be contacted by c2 scripts"
}

cmd_off() {
  rm -f "$MARK"
  log "isolation OFF"
}

cmd_status() {
  if [[ -f "$MARK" ]]; then
    log "isolation ON since $(cat "$MARK")"
  else
    log "isolation OFF"
  fi
  # shellcheck source=cesarops2-isolated.env
  source "$REPO/scripts/cesarops2-isolated.env" 2>/dev/null || true
  log "FORGE_URL=${FORGE_URL:-unset} MODELS=${MODELS:-unset}"
  ps aux | grep -E 'cake run|nauti-inferer|cesarops-forge' | grep -v grep | head -5 || echo "(no forge/cake/nauti processes)"
}

case "${1:-on}" in
  on) cmd_on ;;
  off) cmd_off ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {on|off|status}"
    exit 1
    ;;
esac
