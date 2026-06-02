#!/usr/bin/env bash
# Full fleet hetero: thinker → 3 distinct coders → cross-review → 1070 race → polisher.
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"

STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT:-$REPO/var/role_bench/fleet_hetero_${STAMP}}"
mkdir -p "$OUT"

export OUT
export FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
export THINKER_URL="${THINKER_URL:-http://127.0.0.1:5200}"
export GEMMA_URL="${GEMMA_URL:-http://10.0.0.61:5001}"
export QWEN_URL="${QWEN_URL:-http://10.0.0.61:5002}"
export CODER_1070_URL="${CODER_1070_URL:-http://127.0.0.1:5202}"
export POLISHER_URL="${POLISHER_URL:-http://10.0.0.61:5010}"

echo "[hetero] OUT=$OUT" | tee "$OUT/run.log"
exec python3 "$REPO/scripts/role_bench/run_fleet_hetero_pipeline.py"
