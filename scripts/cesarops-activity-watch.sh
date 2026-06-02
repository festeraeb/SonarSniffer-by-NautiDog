#!/usr/bin/env bash
# Updates last-activity timestamp; triggers cake_fleet after IDLE_SEC of silence.
set -euo pipefail

STATE_DIR="${STATE_DIR:-/home/cesarops/.cache/cesarops}"
ACTIVITY_FILE="${STATE_DIR}/last_activity_ts"
IDLE_SEC="${IDLE_SEC:-600}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FLEET_SCRIPT="${REPO}/scripts/cesarops-fleet-mode.sh"

mkdir -p "$STATE_DIR"
now=$(date +%s)

touch_activity() {
  echo "$now" >"$ACTIVITY_FILE"
}

gpu_active() {
  nvidia-smi --query-gpu=utilization.gpu --format=csv,noheader,nounits 2>/dev/null \
    | awk -v t=10 '$1 > t { found=1 } END { exit !found }'
}

forge_active() {
  curl -sf --max-time 2 "${FORGE_URL}/health" >/dev/null 2>&1 || return 1
  ss -tlnp 2>/dev/null | grep -q ':9100' || return 1
  local busy
  busy=$(curl -sf --max-time 2 "${FORGE_URL}/forge/status" 2>/dev/null | python3 -c "import json,sys; print('1' if json.load(sys.stdin).get('send_busy') else '0')" 2>/dev/null || echo 0)
  [[ "$busy" == "1" ]] && return 0
  return 1
}

# PAMP/n8n completions should not indefinitely block idle — only Forge POSTs reset fully.
if gpu_active || forge_active; then
  touch_activity
  exit 0
fi

if [[ "${1:-}" == "touch" ]]; then
  touch_activity
  exit 0
fi

last=0
[[ -f "$ACTIVITY_FILE" ]] && last=$(cat "$ACTIVITY_FILE")
idle=$((now - last))
if [[ "$idle" -ge "$IDLE_SEC" ]]; then
  bash "$FLEET_SCRIPT" cake_fleet
else
  echo "[activity-watch] idle=${idle}s / ${IDLE_SEC}s"
fi
