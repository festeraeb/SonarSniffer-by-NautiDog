#!/usr/bin/env bash
# Forge preset: Route1 thinker=Mixtral; Route2 corrector=1070 Qwen14; shared P100 Gemma+Qwen.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
export FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"

export THINKER_URL="${THINKER_URL:-http://10.0.0.201:5200}"
export CODER_URL="${CODER_URL:-http://10.0.0.61:5001}"
export REVIEWER_URL="${REVIEWER_URL:-http://10.0.0.61:5002}"
export CORRECTOR_URL="${CORRECTOR_URL:-http://10.0.0.201:5203}"

python3 <<PY
import json, os
from pathlib import Path
forge = Path(os.environ["FORGE_V2_DIR"])
patch = {
    "thinker_endpoint": os.environ["THINKER_URL"],
    "draft_endpoint": os.environ["THINKER_URL"],
    "coder_endpoint": os.environ["CODER_URL"],
    "reviewer_endpoint": os.environ["REVIEWER_URL"],
    "corrector_endpoint": os.environ["CORRECTOR_URL"],
    "chat_agent": "gemma",
    "routing_preset": "dual-lane-mixtral-qwen",
}
for name in ("routing_state.json", "mode_state.json"):
    p = forge / name
    if p.is_file():
        st = json.loads(p.read_text())
        st.update(patch)
        p.write_text(json.dumps(st, indent=2) + "\n")
        print(f"[dual-lane] patched {name}")
PY

curl -sf -X POST "${FORGE_URL}/cluster/routing" -H 'Content-Type: application/json' -d "$(python3 -c "
import json, os
print(json.dumps({
  'thinker_endpoint': os.environ.get('THINKER_URL'),
  'draft_endpoint': os.environ.get('THINKER_URL'),
  'coder_endpoint': os.environ.get('CODER_URL'),
  'reviewer_endpoint': os.environ.get('REVIEWER_URL'),
  'corrector_endpoint': os.environ.get('CORRECTOR_URL'),
  'chat_agent': 'gemma',
}))
")" >/dev/null 2>&1 && echo "[dual-lane] Forge API OK" || echo "[dual-lane] Forge API skip"
