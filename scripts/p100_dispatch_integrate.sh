#!/usr/bin/env bash
# Qwen3.6 MoE on :5001 (+ optional :5002 reviewer) — full integrate dispatch.
set -euo pipefail

REPO="${CESAROPS_PROJECT_ROOT:-/codebase/repos/wreckhunter2000-1}"
LOG="${LOG:-/tmp/p100-qwen-integrate-dispatch.log}"
OUT="${OUT:-$REPO/integrate_out/qwen_moe}"
MAX_INTEGRATE_PER_GPU="${MAX_INTEGRATE_PER_GPU:-40}"
WAIT_SEC="${WAIT_SEC:-240}"

mkdir -p "$OUT/p1000" "$OUT/p1001"
: >"$LOG"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

export P100_CODER_ONLY="${P100_CODER_ONLY:-1}"
export MODEL_CODER="${MODEL_CODER:-/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf}"

need_restore=0
curl -sf --max-time 2 http://127.0.0.1:5001/v1/models >/dev/null || need_restore=1
if curl -sf --max-time 2 http://127.0.0.1:5001/v1/models 2>/dev/null | grep -qi gemma; then
  log "=== Replace Gemma on :5001 with Qwen MoE ==="
  need_restore=1
fi
if [[ "$need_restore" -eq 1 ]]; then
  pkill -f 'llama-server.*--port 5001' 2>/dev/null || true
  pkill -f 'llama-server.*-port 5001' 2>/dev/null || true
  fuser -k 5001/tcp 2>/dev/null || true
  sleep 2
  log "=== Start Qwen3.6 MoE on P100#0 :5001 (coder-only) ==="
  bash "$REPO/scripts/p100_cycle.sh" restore >>"$LOG" 2>&1
else
  log "=== :5001 already up (not Gemma) ==="
fi

log "=== Wait for :5001 (Qwen MoE) ==="
for i in $(seq 1 "$WAIT_SEC"); do
  if curl -sf --max-time 3 http://127.0.0.1:5001/v1/models >/dev/null; then
    log "Qwen :5001 ready (${i}s)"
  curl -sf --max-time 3 http://127.0.0.1:5001/v1/models | head -c 200
  echo
    break
  fi
  sleep 2
  [[ "$i" -eq "$WAIT_SEC" ]] && log "WARN: timeout waiting for :5001"
done

export REPO OUT LOG MAX_INTEGRATE_PER_GPU
export P1000_URL="${P1000_URL:-http://127.0.0.1:5001/v1/chat/completions}"
export P1001_URL="${P1001_URL:-$P1000_URL}"
export P100_CODER_TAG="${P100_CODER_TAG:-QwenMoE:5001}"
export P100_REVIEWER_TAG="${P100_REVIEWER_TAG:-QwenMoE:5001}"

log "=== Dispatch (Qwen MoE: integrate + reference on :5001) ==="
exec python3 "$REPO/scripts/p100_dispatch_integrate.py" 2>&1 | tee -a "$LOG"
