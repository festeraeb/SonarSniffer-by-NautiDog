#!/usr/bin/env bash
# Re-enable mission/gpu-slot LLM recovery after forge_llm_watchdog_off.sh
set -euo pipefail

GUARD="${FORGE_LLM_WATCHDOG_GUARD:-/tmp/cesarops-llm-watchdog-off}"
rm -f "$GUARD"
echo "[llm-watchdog-on] removed guard $GUARD"

systemctl enable --now cesarops-n8n-watchdog.timer >/dev/null 2>&1 || true
echo "[llm-watchdog-on] enabled cesarops-n8n-watchdog.timer (if installed)"
