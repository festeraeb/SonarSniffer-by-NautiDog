#!/usr/bin/env bash
# Dual P100 llama for integrate work (best fit for 16GB each):
#   P100#0 :5001 — Qwen3.6-35B-A3B MoE (coder / bulk integrate)
#   P100#1 :5002 — Qwen3.5-9B MTP (reviewer / ref diffs)
set -euo pipefail
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
export P100_CODER_ONLY=0
export MODEL_CODER="${MODEL_CODER:-/codebase/models/Qwen3.6-35B-A3B-Q4_K_M.gguf}"
export MODEL_REVIEWER="${MODEL_REVIEWER:-/data/cesarops/local_models/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf}"
export LLAMA_FIT_ON=1
LOG="${LOG:-/tmp/p100-dual-integrate.log}"

log() { echo "[p100-dual] $*" | tee -a "$LOG"; }

log "starting dual-P100 llama (MoE :5001 + MTP :5002)…"
bash "$REPO/scripts/p100_cycle.sh" restore >>"$LOG" 2>&1

for i in $(seq 1 120); do
  ok=0
  curl -sf --max-time 2 http://127.0.0.1:5001/v1/models >/dev/null && ok=$((ok + 1))
  curl -sf --max-time 2 http://127.0.0.1:5002/v1/models >/dev/null && ok=$((ok + 1))
  if [[ "$ok" -eq 2 ]]; then
    log "both endpoints ready (${i}s)"
    curl -sf http://127.0.0.1:5001/v1/models | head -c 120; echo
    curl -sf http://127.0.0.1:5002/v1/models | head -c 120; echo
    exit 0
  fi
  sleep 5
done
log "WARN: timeout — check $LOG and /tmp/p100-llama-restore.log"
exit 1
