#!/usr/bin/env bash
# Revert scan-safe mode:
# - Remove the watchdog guard file
# - Re-enable the systemd n8n watchdog timer
set -euo pipefail

GUARD_FILE="${CESAROPS_SCAN_NO_WATCHDOG_GUARD:-/tmp/cesarops-scan-no-watchdog}"

if [[ -f "$GUARD_FILE" ]]; then
  rm -f "$GUARD_FILE"
fi

echo "[scan-safe] guard removed: $GUARD_FILE"

echo "[scan-safe] enabling systemd n8n watchdog timer"
systemctl enable --now cesarops-n8n-watchdog.timer >/dev/null 2>&1 || true

echo "[scan-safe] done"

