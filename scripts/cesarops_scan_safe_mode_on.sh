#!/usr/bin/env bash
# Flip the fleet into "scan-safe" mode:
# - POST Forge into /mode/cesarops (keeps P100s clear)
# - Create a guard file that stops mission_service_watchdog + n8n-watchdog
#   from doing any recovery/model-loading while you scan
# - Disable the systemd n8n watchdog timer (defensive)
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
GUARD_FILE="${CESAROPS_SCAN_NO_WATCHDOG_GUARD:-/tmp/cesarops-scan-no-watchdog}"

mkdir -p "$(dirname "$GUARD_FILE")"
touch "$GUARD_FILE"

echo "[scan-safe] guard= $GUARD_FILE (watchdogs will exit early)"

echo "[scan-safe] switching Forge -> /mode/cesarops at $FORGE_URL"
curl -sf -X POST "$FORGE_URL/mode/cesarops" >/dev/null 2>&1 || {
  echo "[scan-safe] warn: Forge mode switch failed; continuing (guard still active)" >&2
}

echo "[scan-safe] disabling systemd n8n watchdog timer"
systemctl disable --now cesarops-n8n-watchdog.timer >/dev/null 2>&1 || true

echo "[scan-safe] done"

