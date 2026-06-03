#!/usr/bin/env bash
# Dynamic GPU slot watchdog — restore last heartbeat model per port (not triple-stack preset).
#
#   bash scripts/gpu_slot_watchdog.sh tick     # record healthy + recover down
#   bash scripts/gpu_slot_watchdog.sh show     # print heartbeat JSON
#
# Heartbeat file: /data/cesarops/logs/gpu-slot-heartbeat.json (+ repo var/ copy)
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
[[ -d "$REPO/scripts" ]] || REPO="/codebase/repos/wreckhunter2000-1"

export REPO
export FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
export FORGE_ROUTING_STATE="${FORGE_ROUTING_STATE:-$REPO/cesarops-forge-v2/routing_state.json}"
export GPU_SLOT_HEARTBEAT_PATH="${GPU_SLOT_HEARTBEAT_PATH:-/data/cesarops/logs/gpu-slot-heartbeat.json}"
export LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
export CESAROPS2_LAN_HOST="${CESAROPS2_LAN_HOST:-10.0.0.201}"

LOG="${GPU_SLOT_WATCHDOG_LOG:-/data/cesarops/logs/gpu-slot-watchdog.log}"
LOCK="${GPU_SLOT_WATCHDOG_LOCK:-/tmp/gpu-slot-watchdog.lock}"
ISOLATION_MARK="${CESAROPS2_ISOLATION_MARK:-$HOME/.cache/cesarops/cesarops2-isolated}"
SCAN_GUARD="${CESAROPS_SCAN_NO_WATCHDOG_GUARD:-/tmp/cesarops-scan-no-watchdog}"
LLM_GUARD="${FORGE_LLM_WATCHDOG_GUARD:-/tmp/cesarops-llm-watchdog-off}"

mkdir -p "$(dirname "$LOG")" "$(dirname "$GPU_SLOT_HEARTBEAT_PATH")"

if [[ -f "$SCAN_GUARD" || -f "$LLM_GUARD" ]]; then
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] watchdog guard present; skipping gpu_slot_watchdog" | tee -a "$LOG"
  exit 0
fi

log() {
  echo "[$(date '+%Y-%m-%d %H:%M:%S')] $*" | tee -a "$LOG"
}

with_lock() {
  exec 9>"$LOCK"
  if ! flock -n 9; then
    log "already running; exit"
    exit 0
  fi
}

isolation_mode_on() {
  [[ -f "$ISOLATION_MARK" ]]
}

forge_has_active_tasks() {
  local status_json missions_json
  status_json="$(curl -sf --max-time 4 "${FORGE_URL}/forge/status" 2>/dev/null || true)"
  missions_json="$(curl -sf --max-time 4 "${FORGE_URL}/webhook/missions" 2>/dev/null || true)"
  python3 - "$status_json" "$missions_json" <<'PY'
import json, sys
busy = running = False
try:
    s = json.loads(sys.argv[1] or "{}")
    busy = bool(s.get("send_busy"))
except Exception:
    pass
try:
    m = json.loads(sys.argv[2] or "{}")
    arr = m if isinstance(m, list) else (m.get("missions") or [])
    running = any(isinstance(x, dict) and str(x.get("status","")).lower()=="running" for x in arr)
except Exception:
    pass
print("1" if (busy or running) else "0")
PY
}

has_fresh_rebind_ack() {
  local ack="${C2_REBIND_ACK_FILE:-/tmp/c2-rebind.ack}"
  [[ -f "$ack" ]] || return 1
  local now mtime age ttl
  now="$(date +%s)"
  mtime="$(stat -c %Y "$ack" 2>/dev/null || echo 0)"
  ttl="${C2_REBIND_ACK_TTL_SEC:-900}"
  age=$((now - mtime))
  [[ "$age" -ge 0 && "$age" -le "$ttl" ]]
}

should_recover() {
  if isolation_mode_on; then
    return 1
  fi
  local mode in_use ack
  mode="${C2_AUTO_SWITCH_MODE:-idle_only}"
  if [[ "${C2_AUTO_REBIND:-1}" != "1" && "$mode" == "idle_only" ]]; then
    mode="off"
  fi
  in_use="$(forge_has_active_tasks || echo 0)"
  ack=0
  has_fresh_rebind_ack && ack=1
  case "${mode,,}" in
    always)
      [[ "$in_use" == "1" && "$ack" != "1" ]] && return 1
      return 0
      ;;
    idle_only)
      [[ "$in_use" == "1" && "$ack" != "1" ]] && return 1
      return 0
      ;;
    off|*)
      [[ "$ack" == "1" ]] && return 0
      return 1
      ;;
  esac
}

cmd_tick() {
  with_lock
  log "tick start"
  local extra=()
  if ! should_recover; then
    extra+=(--no-recover)
    log "recover deferred (forge busy / policy / isolation)"
  fi
  python3 "$REPO/scripts/gpu_slot_heartbeat.py" tick "${extra[@]}" >>"$LOG" 2>&1 || log "warn: heartbeat tick failed"
  log "tick done"
}

cmd_show() {
  python3 "$REPO/scripts/gpu_slot_heartbeat.py" show
}

case "${1:-tick}" in
  tick) cmd_tick ;;
  show) cmd_show ;;
  *) echo "Usage: $0 {tick|show}"; exit 1 ;;
esac
