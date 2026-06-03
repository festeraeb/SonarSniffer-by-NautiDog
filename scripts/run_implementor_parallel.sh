#!/usr/bin/env bash
# Run multiple dual-lane jobs in parallel (one process per job index).
#
#   JOBS_FILE=scripts/role_bench/dual_lane_jobs_round1.json \
#   JOB_INDICES="0 1" bash scripts/run_implementor_parallel.sh
#
# Prereq: T440 scripts/t440_dual_lane_layout.sh + c2 scripts/cesarops2_dual_lane_layout.sh
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_BASE="${OUT_BASE:-$REPO/var/role_bench/implementor_${STAMP}}"
JOBS_FILE="${JOBS_FILE:-$REPO/scripts/role_bench/dual_lane_jobs_round1.json}"
JOB_INDICES="${JOB_INDICES:-0 1}"
T440="${T440_LAN:-10.0.0.61}"

export REPO T440_LAN="$T440"
# Force RTX thinker — ignore stray MIXTRAL_URL=5211 from CPU bench env
export MIXTRAL_URL="http://127.0.0.1:5200"
export GEMMA_URL="${GEMMA_URL:-http://${T440}:5001}"
export QWEN_URL="${QWEN_URL:-http://${T440}:5010}"
export CODER_P100_URL="${CODER_P100_URL:-http://${T440}:5002}"
export PIPELINE_CHAT_TIMEOUT="${PIPELINE_CHAT_TIMEOUT:-0}"
export FORGE_MEMORY="${FORGE_MEMORY:-1}"
export JOBS_FILE

mkdir -p "$OUT_BASE"
echo "[implementor] OUT_BASE=$OUT_BASE indices=$JOB_INDICES" | tee "$OUT_BASE/orchestrator.log"

pids=()
for idx in $JOB_INDICES; do
  export OUT="$OUT_BASE/job_${idx}"
  export JOB_INDEX="$idx"
  export MAX_JOBS=1
  mkdir -p "$OUT"
  nohup python3 "$REPO/scripts/run_dual_lane_forge_pipeline.py" >>"$OUT/pipeline.log" 2>&1 &
  pids+=($!)
  echo "[implementor] started job_index=$idx pid=${pids[-1]} OUT=$OUT" | tee -a "$OUT_BASE/orchestrator.log"
done

fail=0
for i in "${!pids[@]}"; do
  if ! wait "${pids[$i]}"; then
    echo "[implementor] job ${JOB_INDICES[$i]:-?} exit nonzero" | tee -a "$OUT_BASE/orchestrator.log"
    fail=1
  fi
done

python3 "$REPO/scripts/role_bench/implementor_tracker.py" append \
  round="$STAMP" experiment="parallel_jobs" outcome="done" worked="$([[ $fail -eq 0 ]] && echo true || echo false)" \
  artifacts="$OUT_BASE" notes="indices=$JOB_INDICES"

echo "[implementor] finished OUT_BASE=$OUT_BASE (exit $fail)"
exit "$fail"
