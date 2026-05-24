#!/usr/bin/env bash
# Fleet dispatch: coder → reviewer → coder revision for satellite pipeline gaps.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
OUT="$REPO/cesarops-forge-v2/dispatch_results/satellite_gaps_$(date +%Y%m%d_%H%M%S)"
CODER="${P100_CODER:-http://127.0.0.1:5001}"
REVIEWER="${P100_REVIEWER:-mtp}"
mkdir -p "$OUT"

log() { echo "[fleet] $*"; }

pick_mtp() {
  local pool="${MTP_POOL:-http://10.0.0.201:5571,http://127.0.0.1:5002,http://127.0.0.1:5001}"
  local url id
  IFS=',' read -ra P <<< "$pool"
  for url in "${P[@]}"; do
    url="${url// /}"
    id=$(curl -sf --max-time 4 "${url%/}/v1/models" 2>/dev/null \
      | python3 -c "import sys,json; print(json.load(sys.stdin).get('data',[{}])[0].get('id',''))" 2>/dev/null || true)
    if [[ -n "$id" ]]; then echo "${url%/}"; return 0; fi
  done
  return 1
}

if [[ "$REVIEWER" == "mtp" ]]; then
  REVIEWER="$(pick_mtp)" || REVIEWER="http://127.0.0.1:5002"
fi

agent() {
  local ep="$1" tag="$2" msg="$3"
  log "$tag @ $ep"
  python3 - "$ep" "$msg" <<'PY' | curl -sf --max-time 900 -X POST "$FORGE/cluster/agent/run" \
    -H 'Content-Type: application/json' -d @- >"$OUT/${tag}.json" 2>"$OUT/${tag}.err" || true
import json, sys
ep, msg = sys.argv[1], sys.argv[2]
body = {"message": msg, "safe_mode": True}
if ep in ("mtp", "reviewer"):
    body["role"] = "reviewer"
    body["endpoint"] = "mtp"
else:
    body["endpoint"] = ep
print(json.dumps(body))
PY
  python3 -c "import json; d=json.load(open('$OUT/${tag}.json')); print(d.get('response',d)[:3500])" 2>/dev/null \
    | tee "$OUT/${tag}.txt" || cat "$OUT/${tag}.err" 2>/dev/null | tail -5
  echo
}

TASKS="$(cat <<'EOF'
Implement and verify Great Lakes satellite pipeline gaps (read docs/SATELLITE_PIPELINE_GAPS.md):

1. sat_mission_orchestrator.py — honor gt_wreck_names; dry-run fixture CSV (DONE — verify)
2. Add chip fetch stub: pipelines/satellite/tile_image_fetch.py — given lat/lon + optional download_dir, return base64 PNG path or bytes for detection_scan
3. Wire tile_image_fetch into scripts/run_great_lakes_satellite.sh detection_chain
4. Fix missions/n8n_forge_satellite_pipeline.json spec_path to /codebase/projects/pipelines/...
5. Add Earthdata preflight check function in universal_downloader or sat_mission download stage

Run: DRY_RUN=true python3 sat_mission_orchestrator.py --spec missions/straits_known_wreck_validation.json --dry-run
Expect validate_gt n_pass > 0 after fixture CSV fix.
EOF
)"

REVIEW_PROMPT="Review the coder's satellite pipeline work. Check: gt_wreck_names filter, dry-run CSV fixture, lake_michigan mission JSON, run_great_lakes_satellite.sh, detection tiles with image_b64. List must-fix vs nice-to-have. Reference docs/SATELLITE_PIPELINE_GAPS.md."

REVISE_PROMPT="Apply the reviewer feedback. Make minimal focused diffs only under pipelines/satellite/ and scripts/. Re-run dry-run mentally and state what still blocks DRY_RUN=false live run."

log "=== Round 1: Coder ==="
agent "$CODER" "coder_r1" "$TASKS"

log "=== Round 1: Reviewer ==="
agent "$REVIEWER" "reviewer_r1" "$REVIEW_PROMPT

Coder output excerpt:
$(head -c 2500 "$OUT/coder_r1.txt" 2>/dev/null || echo 'n/a')"

log "=== Round 2: Coder revision ==="
agent "$CODER" "coder_r2" "$REVISE_PROMPT

Reviewer feedback:
$(head -c 2500 "$OUT/reviewer_r1.txt" 2>/dev/null || echo 'n/a')"

log "=== Round 2: Reviewer polish ==="
agent "$REVIEWER" "reviewer_r2" "Polish sign-off: summarize what is merge-ready vs still blocked for live STAC. One paragraph + bullet test plan.

Revision:
$(head -c 2000 "$OUT/coder_r2.txt" 2>/dev/null || echo 'n/a')"

log "Results in $OUT"
