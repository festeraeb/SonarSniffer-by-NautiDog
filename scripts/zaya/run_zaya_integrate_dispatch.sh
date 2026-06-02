#!/usr/bin/env bash
# Same laptop integrate task as Gemma/MoE — Zaya on cesarops2 :8010
set -euo pipefail

REPO="${CESAROPS_PROJECT_ROOT:-/codebase/repos/wreckhunter2000-1}"
OUT="${OUT:-$REPO/integrate_out/zaya}"
LOG="${LOG:-/tmp/zaya-integrate-dispatch.log}"
ZAYA_URL="${ZAYA_URL:-http://10.0.0.201:8010/v1/chat/completions}"
MAX_INTEGRATE_PER_GPU="${MAX_INTEGRATE_PER_GPU:-40}"
WAIT_SEC="${WAIT_SEC:-120}"

mkdir -p "$OUT/p1000" "$OUT/p1001"
: >"$LOG"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

log "=== Wait for Zaya :8010 ==="
for i in $(seq 1 "$WAIT_SEC"); do
  if curl -sf --max-time 3 http://10.0.0.201:8010/health >/dev/null; then
    curl -sf --max-time 3 http://10.0.0.201:8010/health | tee -a "$LOG"
    echo | tee -a "$LOG"
    log "Zaya ready (${i}s)"
    break
  fi
  sleep 2
  [[ "$i" -eq "$WAIT_SEC" ]] && log "WARN: timeout waiting for Zaya"
done

export REPO OUT LOG MAX_INTEGRATE_PER_GPU ZAYA_URL
export ZAYA_TAG="${ZAYA_TAG:-Zaya:8010}"
export ZAYA_TIMEOUT="${ZAYA_TIMEOUT:-3600}"
export ZAYA_MAX_BODY_CHARS="${ZAYA_MAX_BODY_CHARS:-4000}"
export ZAYA_MAX_TOKENS="${ZAYA_MAX_TOKENS:-512}"

log "=== Dispatch (same integrate + reference as P100) ==="
exec python3 "$REPO/scripts/zaya/zaya_dispatch_integrate.py" 2>&1 | tee -a "$LOG"
