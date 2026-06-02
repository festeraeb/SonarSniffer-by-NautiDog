#!/usr/bin/env bash
# OperatorSpec draft bench — cesarops2 RTX (:5200) + 1070 ZAYA (:5203) only.
# Does not use T440 P100s (:5001/:5002).
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/role_bench/run_spec_draft_bench.py" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"

export SPEC_BENCH_HOST="${SPEC_BENCH_HOST:-127.0.0.1}"
export SPEC_BENCH_PORTS="${SPEC_BENCH_PORTS:-5200,5203}"
export SPEC_BENCH_TIMEOUT="${SPEC_BENCH_TIMEOUT:-300}"
export SPEC_BENCH_MAX_TOKENS="${SPEC_BENCH_MAX_TOKENS:-1536}"

log() { echo "[spec-bench] $*"; }

ensure_slots() {
  local ok=0
  for p in ${SPEC_BENCH_PORTS//,/ }; do
    if curl -sf --max-time 3 "http://${SPEC_BENCH_HOST}:${p}/v1/models" >/dev/null; then
      log "up :${p}"
      ok=$((ok + 1))
    else
      log "down :${p}"
    fi
  done
  if [[ "$ok" -lt 1 ]]; then
    log "starting unified c2 layout (RTX draft + ZAYA 1070)"
    FLEET_UNIFIED=1 bash "${REPO}/scripts/cesarops2_unified_layout.sh" start || true
    sleep 5
  fi
}

ensure_slots

exec python3 "${REPO}/scripts/role_bench/run_spec_draft_bench.py" "$@"
