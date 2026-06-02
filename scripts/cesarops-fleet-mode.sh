#!/usr/bin/env bash
# Fleet mode state machine: normal (llama interactive) <-> cake_fleet (idle audit).
# Disabled by default until pipeline complete: touch /etc/cesarops/fleet-mode.enabled
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
STATE_DIR="${STATE_DIR:-/home/cesarops/.cache/cesarops}"
MODE_FILE="${STATE_DIR}/fleet_mode"
STAGED="${REPO}/research_log/staged_fixes"
CAKE_START="${REPO}/scripts/cake/start-fleet-cluster.sh"
CAKE_STOP="${REPO}/scripts/cake/stop-fleet-cluster.sh"
INTAKE_URL="${INTAKE_URL:-http://10.0.0.201:5599}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
IDLE_SEC="${IDLE_SEC:-600}"
ENABLED_FLAG="${ENABLED_FLAG:-/etc/cesarops/fleet-mode.enabled}"

mkdir -p "$STATE_DIR" "$STAGED"

current_mode() {
  [[ -f "$MODE_FILE" ]] && cat "$MODE_FILE" || echo "normal"
}

set_mode() {
  echo "$1" >"$MODE_FILE"
  echo "[fleet-mode] -> $1"
}

mission_running() {
  local missions
  missions=$(curl -sf --max-time 3 "${FORGE_URL}/webhook/missions" 2>/dev/null || echo "[]")
  echo "$missions" | grep -q '"status":"running"' && return 0
  return 1
}

enabled() {
  [[ -f "$ENABLED_FLAG" ]]
}

enter_cake_fleet() {
  if ! enabled; then
    echo "[fleet-mode] disabled ($ENABLED_FLAG missing) — skip enter cake_fleet"
    return 0
  fi
  if mission_running; then
    echo "[fleet-mode] pipeline mission running — skip cake_fleet"
    return 0
  fi
  echo "[fleet-mode] entering cake_fleet"
  bash "${REPO}/scripts/p100_cycle.sh" free || true
  pkill -f 'llama-server.*5200' 2>/dev/null || true
  pkill -f 'llama-server.*5571' 2>/dev/null || true
  if [[ -x "$CAKE_START" ]]; then
    USE_70B="${USE_70B:-0}" bash "$CAKE_START" 2>>"${STATE_DIR}/cake_fleet.log" || true
  else
    echo "[fleet-mode] missing $CAKE_START" >>"${STATE_DIR}/cake_fleet.log"
  fi
  bash "${REPO}/scripts/cesarops-cake-audit-loop.sh" once &
  set_mode "cake_fleet"
}

enter_normal() {
  echo "[fleet-mode] entering normal"
  if [[ -x "$CAKE_STOP" ]]; then
    bash "$CAKE_STOP" 2>>"${STATE_DIR}/cake_fleet.log" || true
  fi
  bash "${REPO}/scripts/p100_cycle.sh" restore || true
  set_mode "normal"
}

case "${1:-status}" in
  status)
    echo "mode=$(current_mode) enabled=$(enabled && echo yes || echo no)"
    ;;
  normal) enter_normal ;;
  cake_fleet) enter_cake_fleet ;;
  wake)
    enter_normal
    curl -sf --max-time 5 "${INTAKE_URL}/health" >/dev/null 2>&1 || true
    ;;
  *)
    echo "Usage: $0 {status|normal|cake_fleet|wake}"
    exit 1
    ;;
esac
