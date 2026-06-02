#!/usr/bin/env bash
# Verify cesarops2 llama-server endpoints match Forge routing_state (NFS-shared).
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d /mnt/t440/codebase/repos/wreckhunter2000-1 ]] && REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"

ROUTING="${REPO}/cesarops-forge-v2/routing_state.json"
HEALTH="${REPO}/var/fleet-health/latest.json"
echo "[sync_llm] routing_state=${ROUTING}"

if [[ -f "${REPO}/scripts/fleet_health_poll.py" ]]; then
  FLEET_HEALTH_PATCH_ROUTING=1 python3 "${REPO}/scripts/fleet_health_poll.py" || true
fi

check_url() {
  local label=$1 url=$2
  if curl -sf --max-time 4 "${url%/}/v1/models" >/dev/null; then
    echo "[sync_llm] OK  ${label} ${url}"
    return 0
  fi
  echo "[sync_llm] DOWN ${label} ${url}" >&2
  return 1
}

ok=0
fail=0
if [[ -f "$ROUTING" ]]; then
  for key in thinker_endpoint corrector_endpoint draft_endpoint reviewer_endpoint; do
    url=$(python3 -c "import json; d=json.load(open('$ROUTING')); print(d.get('$key',''))" 2>/dev/null || echo "")
    [[ -n "$url" ]] || continue
    if check_url "$key" "$url"; then ok=$((ok+1)); else fail=$((fail+1)); fi
  done
else
  echo "[sync_llm] no routing_state.json — probing defaults"
  for spec in "thinker:http://127.0.0.1:5200" "picasso:http://127.0.0.1:5571"; do
    label=${spec%%:*}
    url=${spec#*:}
    if check_url "$label" "$url"; then ok=$((ok+1)); else fail=$((fail+1)); fi
  done
fi

echo "[sync_llm] online=${ok} offline=${fail}"
[[ "$fail" -eq 0 ]]
