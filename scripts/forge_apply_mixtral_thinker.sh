#!/usr/bin/env bash
# Thinker = Mixtral on c2 :5200 (RTX+CPU hybrid). Coders/reviewer stay on T440 P100s.
set -euo pipefail

source "${REPO:-/data/codebase/repos/wreckhunter2000-1}/scripts/lib/fleet_resolve.sh" 2>/dev/null || true
export FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
THINKER="${THINKER_URL:-http://10.0.0.201:5200}"
CODER="${CODER_URL:-http://10.0.0.61:5001}"
REVIEWER="${REVIEWER_URL:-http://10.0.0.61:5002}"

if ! curl -sf --max-time 5 "${THINKER}/v1/models" >/dev/null; then
  echo "[mixtral-thinker] WARN: thinker down at $THINKER — start cesarops2_mixtral_thinker_layout.sh first"
fi

export THINKER_URL="$THINKER" CODER_URL="$CODER" REVIEWER_URL="$REVIEWER" FORGE_V2_DIR="$FORGE"
python3 <<PY
import json, os
from pathlib import Path
thinker = os.environ["THINKER_URL"]
coder = os.environ["CODER_URL"]
reviewer = os.environ["REVIEWER_URL"]
forge = Path(os.environ["FORGE_V2_DIR"])
for name in ("routing_state.json", "mode_state.json"):
    path = forge / name
    if not path.is_file():
        continue
    st = json.loads(path.read_text())
    st["thinker_endpoint"] = thinker
    st["draft_endpoint"] = thinker
    st["coder_endpoint"] = coder
    st["reviewer_endpoint"] = reviewer
    st["corrector_endpoint"] = reviewer
    if "routing_preset" in st:
        st["routing_preset"] = "mixtral-thinker-dual-coder"
    path.write_text(json.dumps(st, indent=2) + "\n")
    print(f"[mixtral-thinker] {name} → thinker {thinker}")
PY

if curl -sf --max-time 5 "${FORGE_URL}/health" >/dev/null; then
  curl -sf -X POST "${FORGE_URL}/cluster/routing" -H 'Content-Type: application/json' -d "$(python3 -c "
import json, os
print(json.dumps({
  'thinker_endpoint': os.environ.get('THINKER_URL','http://10.0.0.201:5200'),
  'draft_endpoint': os.environ.get('THINKER_URL','http://10.0.0.201:5200'),
  'coder_endpoint': 'http://10.0.0.61:5001',
  'reviewer_endpoint': 'http://10.0.0.61:5002',
  'corrector_endpoint': 'http://10.0.0.61:5002',
  'chat_agent': 'gemma',
}))
")" >/dev/null && echo "[mixtral-thinker] Forge API routing updated"
else
  echo "[mixtral-thinker] Forge not reachable at $FORGE_URL"
fi
