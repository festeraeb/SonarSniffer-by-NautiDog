#!/usr/bin/env bash
# Smoke-test Rust satellite orchestrator pipeline on T440.
set -euo pipefail

FORGE="${FORGE:-http://127.0.0.1:9100}"
SPEC="${SPEC:-/codebase/repos/wreckhunter2000-1/pipelines/satellite/missions/straits_known_wreck_validation.json}"
FREE_P100="${FREE_P100:-0}"

log() { echo "[test_pipeline] $*"; }

if [[ "$FREE_P100" == "1" ]]; then
  log "Freeing P100 ports for test..."
  bash "$(dirname "$0")/p100_cycle.sh" free || true
fi

log "1. detection health (tool)"
curl -sf -X POST "$FORGE/tool/detection_health" \
  -H 'Content-Type: application/json' \
  -d '{"arguments":{}}' | head -c 400
echo ""

log "2. plan (spec_path)"
curl -sf -X POST "$FORGE/orchestrator/plan" \
  -H 'Content-Type: application/json' \
  -d "$(cat <<EOF
{
  "raw_text": "wreck hunt straits validation",
  "spec_path": "$SPEC",
  "bbox": [45.6, -85.6, 46.2, -84.3],
  "days_back": 30,
  "dry_run": true,
  "pipeline_mode": "sequential"
}
EOF
)" | python3 -m json.tool | head -60

log "3. execute mission (dry_run, sequential) — may take several minutes"
curl -sf -m 600 -X POST "$FORGE/orchestrator/execute" \
  -H 'Content-Type: application/json' \
  -d "$(cat <<EOF
{
  "raw_text": "wreck hunt straits validation",
  "spec_path": "$SPEC",
  "bbox": [45.6, -85.6, 46.2, -84.3],
  "days_back": 30,
  "dry_run": true,
  "pipeline_mode": "sequential",
  "knobs": {"dry_run_download": true}
}
EOF
)" | python3 -m json.tool | head -80

log "done"

if [[ "$FREE_P100" == "1" ]]; then
  log "Restoring Kobold on P100..."
  bash "$(dirname "$0")/p100_cycle.sh" restore || true
fi
