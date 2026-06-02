#!/usr/bin/env bash
# Apply dual-coder + ZAYA thinker routing (Gemma :5001 + c2 MoE :5200, Qwen :5002 reviewer).
# PRIMARY Forge only: http://127.0.0.1:9100 on cesarops2 — NOT http://10.0.0.61:9100 (deprecated).
set -euo pipefail
export FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
if [[ "${FLEET_UNIFIED:-0}" == "1" || -f "${HOME}/.cache/cesarops/fleet-unified" ]]; then
  export CESAROPS2_ISOLATED=0
  export ALLOW_T440_FLEET=1
else
  export CESAROPS2_ISOLATED=1
fi
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/cesarops-forge-v2" ]] || REPO="/codebase/repos/wreckhunter2000-1"
FORGE="${FORGE_V2_DIR:-$REPO/cesarops-forge-v2}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"

cp "$FORGE/routing/routing_state.dual-coder-zaya.json" "$FORGE/routing_state.json"
cp "$FORGE/routing/mode_state.dual-coder-zaya.json" "$FORGE/mode_state.json"
echo "[dual-coder-zaya] routing_state + mode_state written"

# If ZAYA endpoint is not actually up, fall back to the draft lane so Forge stays usable.
# (ZAYA GGUF may require a dedicated runtime; keep cluster functional during bring-up.)
if ! curl -sf --max-time 3 "http://10.0.0.201:5203/v1/models" >/dev/null 2>&1; then
  python3 - "$FORGE/routing_state.json" <<'PY'
import json, sys
p = sys.argv[1]
st = json.load(open(p))
st["thinker_endpoint"] = st.get("draft_endpoint") or "http://10.0.0.201:5200"
st["validator_zaya"] = st["thinker_endpoint"]
json.dump(st, open(p, "w"), indent=2)
print("[dual-coder-zaya] WARN: ZAYA :5203 down; thinker -> draft_endpoint")
PY
fi

if curl -sf --max-time 5 "${FORGE_URL}/health" >/dev/null; then
  curl -sf -X POST "${FORGE_URL}/cluster/routing/preset/dual-coder-zaya" >/dev/null \
    && echo "[dual-coder-zaya] preset applied via API" \
    || echo "[dual-coder-zaya] API preset skipped (files already correct)"
else
  echo "[dual-coder-zaya] Forge not up — start cesarops-forge-v2 then re-run"
fi
