#!/usr/bin/env bash
# Dual-GPU Rust port dispatch: ~2/3 Qwen :5200, ~1/3 Rust-Coder :5571
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
LOG="${LOG:-/tmp/c2-rust-port-dispatch.log}"

log() { echo "[$(date -u +%H:%M:%S)] $*" | tee -a "$LOG"; }

log "=== wait for :5200 and :5571 ==="
for i in $(seq 1 60); do
  ok=0
  curl -sf --max-time 3 http://127.0.0.1:5200/v1/models >/dev/null && ok=$((ok+1))
  curl -sf --max-time 3 http://127.0.0.1:5571/v1/models >/dev/null && ok=$((ok+1))
  [[ "$ok" -eq 2 ]] && break
  sleep 2
done

export REPO CODER_URL=http://127.0.0.1:5200/v1/chat/completions RUST_URL=http://127.0.0.1:5571/v1/chat/completions
export OUT_QWEN="$REPO/integrate_out/qwen14b_2060" OUT_RUST="$REPO/integrate_out/rust14b_1070"
export LOG MAX_TOKENS=4096 TIMEOUT=900

log "=== dispatch (batch order, 2/3 qwen / 1/3 rust) ==="
exec python3 "$REPO/scripts/integrate/dispatch_c2_rust_port.py" 2>&1 | tee -a "$LOG"
