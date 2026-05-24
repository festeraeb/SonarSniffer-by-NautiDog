#!/usr/bin/env bash
# End-to-end Great Lakes satellite pipeline: Straits + Lake Michigan north.
#
# Usage:
#   DRY_RUN=true  bash scripts/run_great_lakes_satellite.sh   # smoke (default)
#   DRY_RUN=false bash scripts/run_great_lakes_satellite.sh   # live STAC (needs Earthdata)
#   ORCHESTRATOR_ONLY=1 DRY_RUN=true bash ...                  # forge webhook only
#
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
PIPE="${PIPE:-/codebase/projects/pipelines}"
FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
DETECTION_URL="${DETECTION_URL:-http://10.0.0.201:5580}"
DRY_RUN="${DRY_RUN:-true}"
ORCHESTRATOR_ONLY="${ORCHESTRATOR_ONLY:-0}"

SPECS=(
  "$PIPE/satellite/missions/straits_known_wreck_validation.json"
  "$PIPE/satellite/missions/lake_michigan_north_wreck_validation.json"
)

log() { echo "[great-lakes-sat] $*"; }

forge_tool() {
  curl -sf -X POST "$FORGE/tool/$1" -H 'Content-Type: application/json' \
    -d "{\"arguments\":$2}"
}

run_mission_local() {
  local spec="$1"
  log "sat_mission local: $spec dry_run=$DRY_RUN"
  python3 "$PIPE/satellite/sat_mission_orchestrator.py" \
    --spec "$spec" $( [[ "$DRY_RUN" == true || "$DRY_RUN" == 1 ]] && echo --dry-run )
}

run_mission_forge() {
  local spec="$1"
  local name
  name=$(basename "$spec" .json)
  log "orchestrator webhook: $name"
  local payload
  payload=$(DRY_RUN="$DRY_RUN" SPEC="$spec" python3 -c "
import json, os
dry = os.environ.get('DRY_RUN','true').lower() in ('1','true','yes')
print(json.dumps({
  'spec_path': os.environ['SPEC'],
  'raw_text': f'great lakes satellite {os.path.basename(os.environ[\"SPEC\"])}',
  'dry_run': dry,
  'pipeline_mode': 'sequential',
  'knobs': {'dry_run_download': dry},
}))
")
  local mid
  mid=$(curl -sf -X POST "$FORGE/webhook/satellite" -H 'Content-Type: application/json' -d "$payload" \
    | python3 -c "import sys,json; print(json.load(sys.stdin).get('mission_id',''))")
  log "  mission_id=$mid"
  for _ in $(seq 1 90); do
    sleep 5
    local st
    st=$(curl -sf "$FORGE/webhook/missions/$mid" | python3 -c "import sys,json; print(json.load(sys.stdin).get('status',''))" 2>/dev/null || echo "")
    if [[ "$st" == "ok" || "$st" == "partial" || "$st" == "failed" ]]; then
      curl -sf "$FORGE/webhook/missions/$mid" | python3 -m json.tool | head -60
      return 0
    fi
    log "  waiting status=$st"
  done
  log "  TIMEOUT mission $mid"
  return 1
}

detection_chain() {
  local out_dir="$1"
  log "detection chain for $out_dir"
  export DETECTION_URL
  forge_tool detection_health '{}' | python3 -c "import sys,json; print(json.load(sys.stdin).get('result','')[:300])"
  local scan_args
  scan_args=$(python3 -c "
import json
print(json.dumps({
  'region': 'great_lakes_wreck_scan',
  'tiles': [],
  'output_dir': '$out_dir',
}))
")
  # orchestrator enriches tiles from CSV when run via webhook; for manual pass output_dir in enrich - use forge execute instead
  local scan_result
  scan_result=$(forge_tool detection_scan "$(python3 -c "
import json, csv
from pathlib import Path
p = Path('$out_dir') / 'wreck_targeting' / 'wreck_targets_all.csv'
tiles = []
b64 = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mP8z8BQDwAEhQGAhKmMIQAAAABJRU5ErkJggg=='
if p.is_file():
    for row in csv.DictReader(p.open()):
        try:
            tiles.append({'lat': float(row['lat']), 'lon': float(row['lon']), 'image_b64': b64, 'tile_id': row.get('wreck_name','t')})
        except (KeyError, ValueError):
            pass
print(json.dumps({'region': 'great_lakes_wreck_scan', 'tiles': tiles[:16]}))
")")
  echo "$scan_result" | python3 -c "import sys,json; print(json.load(sys.stdin).get('result','')[:400])"
  local job_id
  job_id=$(echo "$scan_result" | python3 -c "import sys,json,re; t=json.load(sys.stdin).get('result',''); m=re.search(r'job_id=([a-f0-9-]+)', t); print(m.group(1) if m else '')" 2>/dev/null || true)
  if [[ -n "$job_id" ]]; then
    sleep 3
    forge_tool detection_poll "$(python3 -c "import json; print(json.dumps({'job_id': '$job_id'}))")" \
      | python3 -c "import sys,json; print(json.load(sys.stdin).get('result','')[:500])"
  fi
}

log "Prerequisites"
curl -sf "$FORGE/health" >/dev/null || { log "Start forge on :9100"; exit 1; }
curl -sf "$DETECTION_URL/health" >/dev/null || log "WARN detection $DETECTION_URL"

for spec in "${SPECS[@]}"; do
  [[ -f "$spec" ]] || { log "SKIP missing $spec"; continue; }
  log "======== $(basename "$spec") ========"
  if [[ "$ORCHESTRATOR_ONLY" == "1" ]]; then
    run_mission_forge "$spec" || true
  else
    run_mission_local "$spec"
    out_dir=$(python3 -c "import json; print(json.load(open('$spec'))['paths']['output_dir'])")
    detection_chain "$out_dir"
  fi
done

log "Done. See docs/SATELLITE_PIPELINE_GAPS.md"
