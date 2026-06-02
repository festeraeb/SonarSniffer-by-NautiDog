#!/usr/bin/env bash
# Installed on cesarops2 as ~/bin/forge-sync-state.sh (forge-sync-state.timer).
set -euo pipefail
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
FORGE_DIR="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
MODE_FILE="$FORGE_DIR/mode_state.json"
ROUTE_FILE="$FORGE_DIR/routing_state.json"
PRESET_SNAP="$FORGE_DIR/routing/mode_state.dual-coder-zaya.json"
ROUTE_SNAP="$FORGE_DIR/routing/routing_state.dual-coder-zaya.json"

_hn="$(hostname -s | tr '[:upper:]' '[:lower:]')"
[[ "$_hn" == *cesarops2* ]] || exit 0
[[ "$_hn" == *t440* ]] && exit 0
[[ -f "$PRESET_SNAP" && -f "$ROUTE_SNAP" ]] || exit 1

changed=0
diff -q "$MODE_FILE" "$PRESET_SNAP" >/dev/null 2>&1 || { cp -a "$PRESET_SNAP" "$MODE_FILE"; changed=1; }
diff -q "$ROUTE_FILE" "$ROUTE_SNAP" >/dev/null 2>&1 || { cp -a "$ROUTE_SNAP" "$ROUTE_FILE"; changed=1; }

curl -sf --max-time 6 -X POST "${FORGE_URL}/cluster/routing/preset/dual-coder-zaya" >/dev/null 2>&1 || true

if [[ "$changed" -eq 1 ]]; then
  busy=$(curl -sf --max-time 2 "${FORGE_URL}/forge/status" 2>/dev/null | python3 -c "import sys,json; print(json.load(sys.stdin).get('send_busy',False))" 2>/dev/null || echo false)
  [[ "$busy" == "True" ]] || sudo systemctl restart cesarops-forge-v2.service 2>/dev/null || true
fi
