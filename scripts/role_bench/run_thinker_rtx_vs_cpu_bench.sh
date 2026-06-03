#!/usr/bin/env bash
set -euo pipefail
REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
export REPO RTX_THINKER_URL="${RTX_THINKER_URL:-http://127.0.0.1:5200}"
export CPU_THINKER_URL="${CPU_THINKER_URL:-http://127.0.0.1:5211}"
export REVIEWER_URL="${REVIEWER_URL:-http://10.0.0.61:5001}"
exec python3 "$REPO/scripts/role_bench/run_thinker_rtx_vs_cpu_bench.py"
