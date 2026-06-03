#!/usr/bin/env bash
# Mixtral :5200 thinker; Gemma :5001 + Qwen :5002 coders; Qwen2.5-Coder-14B :5203 fast/corrector lane.
set -euo pipefail

source "${REPO:-/data/codebase/repos/wreckhunter2000-1}/scripts/lib/fleet_resolve.sh" 2>/dev/null || true
export FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"

THINKER="${THINKER_URL:-http://10.0.0.201:5200}"
CODER_A="${CODER_A_URL:-http://10.0.0.61:5001}"
CODER_B="${CODER_B_URL:-http://10.0.0.61:5002}"
CODER_C="${CODER_C_URL:-http://10.0.0.201:5203}"

export THINKER_URL="$THINKER" CODER_A_URL="$CODER_A" CODER_B_URL="$CODER_B" CODER_C_URL="$CODER_C" FORGE_V2_DIR="$FORGE"
python3 <<'PY'
import json, os
from pathlib import Path

thinker = os.environ["THINKER_URL"]
coder_a = os.environ["CODER_A_URL"]
coder_b = os.environ["CODER_B_URL"]
coder_c = os.environ["CODER_C_URL"]
forge = Path(os.environ["FORGE_V2_DIR"])

patch = {
    "thinker_endpoint": thinker,
    "draft_endpoint": thinker,
    "coder_endpoint": coder_a,
    "reviewer_endpoint": coder_b,
    "corrector_endpoint": coder_c,
    "chat_agent": "gemma",
}
if os.environ.get("FORGE_EXTRA_JSON"):
    patch.update(json.loads(os.environ["FORGE_EXTRA_JSON"]))

for name in ("routing_state.json", "mode_state.json"):
    path = forge / name
    if not path.is_file():
        continue
    st = json.loads(path.read_text())
    st.update(patch)
    if "routing_preset" in st:
        st["routing_preset"] = "mixtral-gemma-qwen-1070"
    path.write_text(json.dumps(st, indent=2) + "\n")
    print(f"[mixtral-gwq] {name} thinker={thinker} coders={coder_a} {coder_b} 1070={coder_c}")
PY

if curl -sf --max-time 5 "${FORGE_URL}/health" >/dev/null 2>&1 || curl -sf --max-time 5 "${FORGE_URL}/forge/status" >/dev/null 2>&1; then
  curl -sf -X POST "${FORGE_URL}/cluster/routing" -H 'Content-Type: application/json' -d "$(python3 -c "
import json, os
print(json.dumps({
  'thinker_endpoint': os.environ.get('THINKER_URL','http://10.0.0.201:5200'),
  'draft_endpoint': os.environ.get('THINKER_URL','http://10.0.0.201:5200'),
  'coder_endpoint': 'http://10.0.0.61:5001',
  'reviewer_endpoint': 'http://10.0.0.61:5002',
  'corrector_endpoint': 'http://10.0.0.201:5203',
  'chat_agent': 'gemma',
}))
")" >/dev/null && echo "[mixtral-gwq] Forge API routing updated at $FORGE_URL"
else
  echo "[mixtral-gwq] Forge not reachable at $FORGE_URL (local routing_state.json still patched)"
fi
