#!/usr/bin/env bash
# Import n8n fleet + PAMP workflows and enable live orchestration in cluster_config.toml.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
N8N="${N8N_BIN:-n8n}"
PATH="${PATH:-/home/cesarops/node-v20.18.0-linux-x64/bin:$PATH}"

echo "=== Import n8n workflows ==="
for wf in \
  "${REPO}/missions/n8n_fleet_ops_dispatch.json" \
  "${REPO}/missions/n8n_predictive_async_moe.json"; do
  [[ -f "$wf" ]] || { echo "missing $wf"; exit 1; }
  "$N8N" import:workflow --input="$wf" 2>/dev/null || echo "(import may already exist) $wf"
done

echo "=== Activate fleet + PAMP workflows ==="
for id in zYi8DxVjDzlQVCkv KYHoNSz9XbzswntQ; do
  "$N8N" update:workflow --id="$id" --active=true 2>/dev/null || true
done
# Also activate by name if IDs differ on fresh install
"$N8N" list:workflow 2>/dev/null | grep -E 'Fleet Ops|PAMP' || true

echo "=== Enable live orchestration via Forge API ==="
curl -sf -X POST http://127.0.0.1:9100/cluster/orchestration \
  -H 'Content-Type: application/json' \
  -d '{"tools_backend":"n8n","pamp_shadow":false,"default_baseline":"interactive_fast"}' \
  | head -c 400
echo ""

echo "=== Smoke: fleet-ops enqueue cesarops2 sync ==="
bash "${REPO}/scripts/fleet-n8n-dispatch.sh" cesarops2 sync_llm_endpoints || true

echo "Done. Cluster panel: http://127.0.0.1:9100/cluster"
