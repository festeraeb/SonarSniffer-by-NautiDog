#!/usr/bin/env bash
# Resume satellite pipeline — P100 orchestration + cesarops2 augment.
#
# Flow: plan → P100 review → remember (vector log) → sat_mission → detection (c2)
#
# P100 roles (dual-coding-split):
#   :5001 Gemma — orchestrator / implement
#   :5002 Qwen  — reviewer / polish
#
# Augment (cesarops2):
#   :5200/:5571 LLM optional
#   :5580 detection triple-lock
#
set -euo pipefail

FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
SPEC="${SPEC:-/codebase/projects/pipelines/satellite/missions/straits_known_wreck_validation.json}"
DETECTION_URL="${DETECTION_URL:-http://10.0.0.201:5580}"
P100_CODER="${P100_CODER:-http://127.0.0.1:5001}"
# MTP pool: cesarops2 augment first (matches forge [endpoint_pool.mtp])
MTP_POOL="${MTP_POOL:-http://10.0.0.201:5571,http://10.0.0.200:5571,http://10.0.0.201:5200,http://10.0.0.200:5200,http://127.0.0.1:5002,http://127.0.0.1:5001}"
P100_REVIEWER="${P100_REVIEWER:-}"
DRY_RUN="${DRY_RUN:-true}"
# Set ORCHESTRATOR=1 to run the full forge sequential pipeline (webhook) instead of manual tools.
ORCHESTRATOR="${ORCHESTRATOR:-0}"

log() { echo "[sat-pipeline] $*"; }

pick_mtp_endpoint() {
  local url id
  IFS=',' read -ra POOL <<< "$MTP_POOL"
  for url in "${POOL[@]}"; do
    url="${url// /}"
    id=$(curl -sf --max-time 3 "${url%/}/v1/models" 2>/dev/null \
      | python3 -c "import sys,json;d=json.load(sys.stdin);print(d.get('data',[{}])[0].get('id',''))" 2>/dev/null || true)
    if [[ -n "$id" ]] && [[ "$id" == *MTP* || "$id" == *mtp* ]]; then
      echo "${url%/}"
      return 0
    fi
    if [[ -n "$id" ]]; then
      log "skip non-MTP $url ($id)"
    fi
  done
  return 1
}

if [[ -z "$P100_REVIEWER" ]]; then
  P100_REVIEWER="$(pick_mtp_endpoint)" || P100_REVIEWER="http://10.0.0.201:5571"
  log "MTP reviewer endpoint -> $P100_REVIEWER"
fi

forge_tool() {
  local name="$1" args="$2"
  curl -sf -X POST "$FORGE/tool/$name" \
    -H 'Content-Type: application/json' \
    -d "{\"arguments\":$args}" | python3 -c "
import sys,json
d=json.load(sys.stdin)
print(d.get('result',d))
"
}

forge_agent() {
  local endpoint="$1" msg="$2"
  python3 - "$endpoint" "$msg" <<'PY' | curl -sf --max-time 600 -X POST "$FORGE/cluster/agent/run" \
    -H 'Content-Type: application/json' -d @- \
    | python3 -c "import sys,json; d=json.load(sys.stdin); print(str(d.get('response',d))[:4000])"
import json, sys
print(json.dumps({
  "endpoint": sys.argv[1],
  "message": sys.argv[2],
  "safe_mode": True,
}))
PY
}

# ── 0. Prerequisites ─────────────────────────────────────────────────────
log "Forge + augment probes"
curl -sf "$FORGE/health" >/dev/null || { log "Start forge first"; exit 1; }
curl -sf "$DETECTION_URL/health" >/dev/null || log "WARN: detection at $DETECTION_URL not up"

# Fix pipelines symlink if broken
if [[ ! -f "$REPO/pipelines/satellite/sat_mission_orchestrator.py" ]]; then
  if [[ -f /codebase/projects/pipelines/satellite/sat_mission_orchestrator.py ]]; then
    ln -sfn /codebase/projects/pipelines "$REPO/pipelines"
    log "Fixed pipelines symlink -> /codebase/projects/pipelines"
  fi
fi

export DETECTION_URL

if [[ "$ORCHESTRATOR" == "1" || "$ORCHESTRATOR" == "true" ]]; then
  log "Full orchestrator mode — POST /webhook/satellite (sequential pipeline + MTP polish)"
  curl -sf -X POST "$FORGE/cluster/routing/preset/dual-coding-split" >/dev/null || true
  PAYLOAD=$(DRY_RUN="$DRY_RUN" SPEC="$SPEC" python3 -c "
import json, os
dry = os.environ.get('DRY_RUN','true').lower() in ('1','true','yes')
print(json.dumps({
  'spec_path': os.environ['SPEC'],
  'dry_run': dry,
  'knobs': {'dry_run_download': dry, 'max_wrecks': 4},
}))
")
  MISSION=$(curl -sf -X POST "$FORGE/webhook/satellite" \
    -H 'Content-Type: application/json' \
    -d "$PAYLOAD")
  MID=$(echo "$MISSION" | python3 -c "import sys,json; print(json.load(sys.stdin).get('mission_id',''))")
  log "mission_id=$MID — polling /webhook/missions/$MID"
  for _ in $(seq 1 120); do
    sleep 5
    STATUS=$(curl -sf "$FORGE/webhook/missions/$MID" 2>/dev/null || echo '{}')
  ST=$(echo "$STATUS" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || true)
    if [[ "$ST" == "ok" || "$ST" == "partial" || "$ST" == "failed" ]]; then
      echo "$STATUS" | python3 -m json.tool 2>/dev/null | head -80
      exit 0
    fi
    log "  status=$ST ..."
  done
  log "Timed out waiting for mission $MID"
  exit 1
fi

# ── 1. P100 routing preset ───────────────────────────────────────────────
log "Apply dual-coding-split (coder T440 :5001, reviewer MTP cesarops2)"
curl -sf -X POST "$FORGE/cluster/routing/preset/dual-coding-split" >/dev/null || true

# ── 2. Orchestrator plan (heuristic + optional intake) ─────────────────────
log "Mission plan (forge orchestrator)"
PLAN=$(curl -sf -X POST "$FORGE/orchestrator/plan" \
  -H 'Content-Type: application/json' \
  -d '{
    "raw_text": "satellite wreck validation Straits of Mackinac — Sentinel targeting + GT known wrecks",
    "bbox": [45.6, -85.6, 46.2, -84.3],
    "days_back": 30,
    "spec_path": "'"$SPEC"'"
  }')
echo "$PLAN" | python3 -m json.tool 2>/dev/null | head -40 || echo "$PLAN" | head -c 1500
echo

# ── 3. MTP review (cesarops2 :5571 / :5200 first, then T440) ─────────────────
log "MTP reviewer pass @ $P100_REVIEWER"
REVIEW=$(forge_agent "$P100_REVIEWER" "Review this satellite mission plan for Great Lakes wreck detection. List gaps, timing risks (zebra mussel clarity, post-storm plumes), and which stages to enable first. Plan context: straits_known_wreck_validation.json stages download,target_known,temporal_stack,validate_gt,report.")
echo "$REVIEW" | head -c 2000
echo

# ── 4. Log to vector / lessons (remember) ──────────────────────────────────
log "remember → nautivecs + research_log"
forge_tool remember "$(python3 -c "
import json
print(json.dumps({
  'content': 'Satellite pipeline resume: straits GT validation. Review notes stored.',
  'tags': 'satellite,pipeline,straits,review,orchestrator'
}))
")" | head -c 500
echo

# ── 5. Run sat_mission (JSON knobs — dry-run default) ──────────────────────
log "sat_mission spec=$SPEC dry_run=$DRY_RUN"
SAT_ARGS=$(DRY_RUN="$DRY_RUN" SPEC="$SPEC" python3 -c "
import json, os
dry = os.environ['DRY_RUN'].lower() in ('1','true','yes')
print(json.dumps({
  'spec_path': os.environ['SPEC'],
  'dry_run': dry,
  'knobs': {'dry_run_download': dry, 'max_wrecks': 4},
}))
")
forge_tool sat_mission "$SAT_ARGS" | tee "/tmp/sat_mission_$(date +%Y%m%d_%H%M%S).log" | tail -40
echo

# ── 6. Detection health via cesarops2 ──────────────────────────────────────
log "detection_health (DETECTION_URL=$DETECTION_URL)"
forge_tool detection_health '{}' 
echo

# ── 7. P100 implement notes (Gemma on :5001) ───────────────────────────────
log "P100 coder — polish / implement next steps"
forge_agent "$P100_CODER" "Based on sat_mission output and review, list the top 3 code changes to wire targeting CSV into detection_scan tiles. Be specific file paths under wreckhunter2000-1. No fluff." \
  | head -c 2500

log "Done. Set DRY_RUN=false for live STAC download."
