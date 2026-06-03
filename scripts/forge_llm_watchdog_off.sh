#!/usr/bin/env bash
# Stop mission/gpu-slot watchdogs from reloading LLM models on P100 ports.
# Use when :5002 must stay Qwen2.5-Coder-14B (watchdog was restoring Qwen3.6 MoE snapshots).
#
#   bash scripts/forge_llm_watchdog_off.sh
#   bash scripts/start_qwen14_coder_p100.sh
#   bash scripts/pin_qwen14_heartbeat_5002.sh   # optional: fix heartbeat store
#
# Re-enable:
#   bash scripts/forge_llm_watchdog_on.sh
set -euo pipefail

GUARD="${FORGE_LLM_WATCHDOG_GUARD:-/tmp/cesarops-llm-watchdog-off}"
mkdir -p "$(dirname "$GUARD")"
touch "$GUARD"

echo "[llm-watchdog-off] guard=$GUARD"
echo "  mission_service_watchdog.sh — skipped"
echo "  gpu_slot_watchdog.sh — skipped"
echo "  n8n-watchdog LLM handoffs — reduced (guard checked)"

systemctl disable --now cesarops-n8n-watchdog.timer >/dev/null 2>&1 || true
echo "[llm-watchdog-off] disabled cesarops-n8n-watchdog.timer (if installed)"

echo "[llm-watchdog-off] start Qwen14 manually:"
echo "  bash scripts/start_qwen14_coder_p100.sh"
echo "  bash scripts/pin_qwen14_heartbeat_5002.sh"
