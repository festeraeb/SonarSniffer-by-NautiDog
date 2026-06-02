#!/usr/bin/env bash
# Prep cesarops2 hetero workers: 72B cache, then :10129 (2060/sm75) + :10130 (1070/sm61). P106 out by default.
# Run on cesarops2 after stopping competing cake worker downloads.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
export CAKE_CLUSTER_KEY_FILE="${CAKE_CLUSTER_KEY_FILE:-${HOME}/.cache/cesarops/cake-cluster.key}"
export USE_70B=1
export SKIP_P106="${SKIP_P106:-1}"
export CAKE_MODEL="${CAKE_MODEL:-Qwen/Qwen2.5-72B-Instruct}"
# shellcheck source=fleet-env.sh
source "${SCRIPT_DIR}/fleet-env.sh"

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
CAKE_SM75="${CAKE_SM75:-/opt/cesarops/cake/bin/cake-sm75}"
CAKE_SM61="${CAKE_SM61:-/opt/cesarops/cake/bin/cake-sm61}"
[[ -x "$CAKE_SM75" ]] || CAKE_SM75="$CAKE"
[[ -x "$CAKE_SM61" ]] || CAKE_SM61="$CAKE"
MODEL="${CAKE_MODEL}"
HF_BIN="${HF_BIN:-${HOME}/.venvs/hf/bin/hf}"
WAIT_CACHE="${WAIT_CACHE:-1}"

log() { echo "[c2-prep] $*" | tee -a "$CAKE_LOG"; }

shard_count() {
  python3 <<'PY'
import json, pathlib
hub = pathlib.Path.home() / ".cache/huggingface/hub/models--Qwen--Qwen2.5-72B-Instruct/snapshots"
snaps = sorted(hub.glob("*/model.safetensors.index.json"), key=lambda p: p.stat().st_mtime, reverse=True)
if not snaps:
    print("0 0")
    raise SystemExit(0)
idx = json.loads(snaps[0].read_text())
files = set(idx["weight_map"].values())
have = len(list(snaps[0].parent.glob("model-*.safetensors")))
print(len(files), have)
PY
}

wait_for_cache() {
  local total have
  read -r total have < <(shard_count)
  log "cache ${have}/${total} shards for ${MODEL}"
  if [[ "$have" -ge "$total" && "$total" -gt 0 ]]; then
    return 0
  fi
  if [[ "$WAIT_CACHE" != "1" ]]; then
    log "incomplete cache and WAIT_CACHE=0 — continuing anyway"
    return 0
  fi
  if [[ -x "$HF_BIN" ]]; then
    log "pulling missing shards via hf download (single process)…"
    "$HF_BIN" download "$MODEL" --max-workers 4 >>"$CAKE_LOG" 2>&1 || true
  elif command -v "$CAKE" >/dev/null 2>&1 || [[ -x "$CAKE" ]]; then
    log "pulling via cake pull…"
    "$CAKE" pull "$MODEL" >>"$CAKE_LOG" 2>&1 || true
  fi
  local deadline=$((SECONDS + 7200))
  while (( SECONDS < deadline )); do
    read -r total have < <(shard_count)
    log "cache ${have}/${total} shards…"
    [[ "$have" -ge "$total" && "$total" -gt 0 ]] && return 0
    sleep 30
  done
  log "WARN: cache still incomplete after wait"
}

clean_hf_locks() {
  find "${HOME}/.cache/huggingface/hub/models--Qwen--Qwen2.5-72B-Instruct/blobs" \
    -name '*.lock' -mmin +10 -delete 2>/dev/null || true
}

stop_c2_workers() {
  pkill -f 'cake worker' 2>/dev/null || true
  sleep 2
}

start_c2_workers() {
  mkdir -p "$CAKE_PID_DIR"
  start_w() {
    local bin=$1 dev=$2 port=$3 name=$4
    setsid "$bin" worker --model "$MODEL" --device "$dev" --name "$name" \
      --address "0.0.0.0:${port}" >>"$CAKE_LOG" 2>&1 &
    echo $! >"${CAKE_PID_DIR}/worker-${name}.pid"
    log "started ${name} bin=$(basename "$bin") gpu=${dev} :${port} pid=$(cat "${CAKE_PID_DIR}/worker-${name}.pid")"
  }
  if [[ "${SKIP_P106}" != "1" ]]; then
    start_w "$CAKE_SM61" 0 10128 c2-p106
  fi
  start_w "$CAKE_SM75" 1 10129 c2-rtx
  start_w "$CAKE_SM61" 2 10130 c2-1070
  log "workers launching — ports bind after model load (check: ss -tlnp | grep 1012)"
}

main() {
  if ! [[ -x "$CAKE" ]]; then
    log "missing cake binary: $CAKE"
    exit 1
  fi
  stop_c2_workers
  clean_hf_locks
  wait_for_cache
  clean_hf_locks
  start_c2_workers
}

main "$@"
