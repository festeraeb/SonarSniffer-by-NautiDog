#!/usr/bin/env bash
# Dual-lane Forge (same 3-step flow; physical layout swapped):
#   Lane A: Mixtral :5200 → Gemma :5001 → Mixtral
#   Lane B: Qwen3.6 CPU :5010 → Qwen14 :5002 → Qwen3.6 CPU :5010
#
#   On T440: bash scripts/t440_dual_lane_layout.sh start
#   On c2:   bash scripts/cesarops2_dual_lane_layout.sh start
#   TASK="..." bash scripts/run_dual_lane_forge_pipeline.sh
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT:-$REPO/var/role_bench/dual_lane_${STAMP}}"

export OUT REPO
export MIXTRAL_URL="${MIXTRAL_URL:-http://127.0.0.1:5200}"
export GEMMA_URL="${GEMMA_URL:-http://10.0.0.61:5001}"
export T440_LAN="${T440_LAN:-10.0.0.61}"
export QWEN_URL="${QWEN_URL:-http://${T440_LAN}:5010}"
export CODER_P100_URL="${CODER_P100_URL:-http://${T440_LAN}:5002}"
export FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"
export JOBS_FILE="${JOBS_FILE:-$REPO/scripts/role_bench/dual_lane_jobs_round1.json}"
export JOB_INDEX="${JOB_INDEX:-0}"
export MAX_JOBS="${MAX_JOBS:-1}"

mkdir -p "$OUT"
echo "[dual-lane] OUT=$OUT" | tee "$OUT/run.log"
exec python3 "$REPO/scripts/run_dual_lane_forge_pipeline.py"
