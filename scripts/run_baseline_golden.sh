#!/usr/bin/env bash
# Phase 5 — golden tasks for B5/B3/B7; logs to model scorecard + audit_runs.jsonl
set -euo pipefail

FORGE="${FORGE_URL:-http://127.0.0.1:9100}"
METRICS="${HOME}/.cache/cesarops/audit_runs.jsonl"
mkdir -p "$(dirname "$METRICS")"

run_task() {
  local baseline=$1 label=$2 prompt=$3
  local t0=$(date +%s%N)
  local resp
  resp=$(curl -sf --max-time 600 -X POST "${FORGE}/send" \
    -H 'Content-Type: application/json' \
    -d "{\"message\":\"${prompt}\"}" 2>/dev/null || echo '{"error":"timeout"}')
  local t1=$(date +%s%N)
  local ms=$(( (t1 - t0) / 1000000 ))
  echo "{\"kind\":\"golden\",\"baseline\":\"${baseline}\",\"label\":\"${label}\",\"ms\":${ms}}" >>"$METRICS"
  echo "[$baseline] $label ${ms}ms"
}

echo "=== B5 interactive_fast (serial /send) ==="
CESAROPS_BASELINE=interactive_fast run_task B5 rust_fn 'Write a Rust fn add(a:i32,b:i32)->i32 with a unit test.'

echo "=== B3 conductor (plan-style prompt) ==="
run_task B3 plan 'Plan: refactor loop_engine corrector cascade into a trait. Bullet steps only.'

echo "=== B7 multi-step ==="
run_task B7 execute 'Execute: list three files in cesarops-forge-v2/src and summarize each in one line.'

if curl -sf --max-time 5 http://127.0.0.1:5678/healthz >/dev/null 2>&1; then
  echo "=== PAMP shadow via n8n ==="
  curl -sf -X POST http://127.0.0.1:5678/webhook/pamp-route \
    -H 'Content-Type: application/json' \
    -d '{"message":"Golden PAMP shadow test","mode":"chat","shadow":true}' \
    | head -c 400
  echo ""
fi

echo "Done. Metrics: $METRICS"
