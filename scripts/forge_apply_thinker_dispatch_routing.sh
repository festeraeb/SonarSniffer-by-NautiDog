#!/usr/bin/env bash
# Forge routing for RTX thinker + three coders (no polisher). Does not start workers.
set -euo pipefail

FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
THINKER="${THINKER_URL:-http://10.0.0.201:5200}"
CODER_A="${CODER_A_URL:-http://10.0.0.61:5001}"
CODER_B="${CODER_B_URL:-http://10.0.0.61:5002}"
CODER_C="${CODER_C_URL:-http://10.0.0.201:5202}"

curl -sf -X POST "${FORGE_URL}/cluster/routing" \
  -H 'Content-Type: application/json' \
  -d "$(cat <<EOF
{
  "chat_agent": "gemma",
  "thinker_endpoint": "${THINKER}",
  "coder_endpoint": "${CODER_A}",
  "draft_endpoint": "${CODER_B}",
  "corrector_endpoint": "${CODER_C}",
  "reviewer_endpoint": "${CODER_B}"
}
EOF
)"

echo "Forge routing saved:"
curl -sf "${FORGE_URL}/cluster/routing" | python3 -c "
import json,sys
r=json.load(sys.stdin).get('routing_state',json.load(sys.stdin))
for k in ['thinker_endpoint','coder_endpoint','draft_endpoint','corrector_endpoint','reviewer_endpoint']:
    print(f'  {k}: {r.get(k,\"\")}')
"
