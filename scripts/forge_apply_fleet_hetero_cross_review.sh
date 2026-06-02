#!/usr/bin/env bash
# Forge preset: fleet hetero + cross-review bench (orchestration in run_fleet_hetero_pipeline.sh).
set -euo pipefail

FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"

if curl -sf -X POST "${FORGE_URL}/cluster/routing/preset/fleet-hetero-cross-review" \
  -H 'Content-Type: application/json' >/dev/null 2>&1; then
  echo "Applied preset fleet-hetero-cross-review"
  curl -sf "${FORGE_URL}/cluster/routing" | python3 -c "
import json,sys
r=json.load(sys.stdin).get('routing_state',{})
for k in ['thinker_endpoint','coder_endpoint','draft_endpoint','corrector_endpoint','reviewer_endpoint']:
    print(f'  {k}: {r.get(k,\"\")}')
"
  exit 0
fi

echo "Preset missing — applying manual routing (cross-review pipeline uses bench script):"
curl -sf -X POST "${FORGE_URL}/cluster/routing" \
  -H 'Content-Type: application/json' \
  -d '{
  "chat_agent": "gemma",
  "thinker_endpoint": "http://10.0.0.201:5200",
  "coder_endpoint": "http://10.0.0.61:5001",
  "draft_endpoint": "http://10.0.0.61:5002",
  "corrector_endpoint": "http://10.0.0.201:5202",
  "reviewer_endpoint": "http://10.0.0.61:5002"
}'
echo ""
echo "  thinker   :5200  Gemma E4B (RTX)"
echo "  coder     :5001  Gemma MoE (P100) — cross-reviews Qwen"
echo "  draft     :5002  Qwen3.6 (P100)   — cross-reviews Gemma"
echo "  corrector :5202  Qwen2.5-Coder-7B (1070)"
echo "  polisher  :5010  Coder-Next CPU (bench script only)"
echo ""
echo "Run: bash scripts/role_bench/run_fleet_hetero_pipeline.sh"
