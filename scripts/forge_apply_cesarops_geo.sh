#!/usr/bin/env bash
# Apply CESAROPS geo fleet routing: Gemma codes on T440, graded handoff to c2 MoE reviewer.
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
C2="${C2_HOST:-10.0.0.201}"

echo "[geo-fleet] T440 llama (if needed)…"
bash "$REPO/scripts/p100_gemma_r1_dual.sh" status || bash "$REPO/scripts/p100_gemma_r1_dual.sh" start

echo "[geo-fleet] cesarops2 triple stack (on $C2 — run there if DOWN)…"
for port in 5200 5201 5202; do
  if curl -sf --max-time 3 "http://${C2}:${port}/v1/models" >/dev/null; then
    echo "  OK :$port"
  else
    echo "  DOWN :$port — on cesarops2: bash scripts/cesarops2_triple_gpu_llama.sh start"
  fi
done

cp "$REPO/cesarops-forge-v2/routing/routing_state.cesarops-geo.json" \
  "$REPO/cesarops-forge-v2/routing_state.json"

if curl -sf --max-time 3 "$FORGE/health" >/dev/null; then
  curl -sf -X POST "$FORGE/cluster/routing/preset/cesarops-geo-fleet" \
    -H 'Content-Type: application/json' \
    -d '{"start_workers":false}' | python3 -m json.tool 2>/dev/null || true
  echo
  curl -sf "$FORGE/cluster/routing" | python3 -c "
import sys,json
r=json.load(sys.stdin).get('resolved',{})
print('coder', r.get('coder_url'))
print('thinker', r.get('thinker_url'))
print('reviewer', r.get('reviewer_url'))
print('corrector', r.get('corrector_url'))
print('parallel_dual_grade', r.get('parallel_dual_grade'))
print('grade_rounds', r.get('parallel_dual_grade_rounds'))
"
else
  echo "[geo-fleet] Forge not on :9100 — routing_state.json updated; start forge and:"
  echo "  curl -X POST $FORGE/cluster/routing/preset/cesarops-geo-fleet -H 'Content-Type: application/json' -d '{\"start_workers\":false}'"
fi

echo "[geo-fleet] Path A → general (:5001). parallel_dual runs Gemma + c2 MoE on rounds 1,2,4,8; R1-32B grades."
