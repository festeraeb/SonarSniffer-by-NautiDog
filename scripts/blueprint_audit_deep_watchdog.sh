#!/usr/bin/env bash
# Deep blueprint fleet watchdog:
# - Starts from scratch (optional clear)
# - Dispatches only unresolved batches
# - Retries failed batches with adaptive chunking
# - Re-checks route health each round
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
PARALLEL="${PARALLEL:-3}"
CHUNK_SIZE="${CHUNK_SIZE:-18}"
MIN_CHUNK_SIZE="${MIN_CHUNK_SIZE:-6}"
MAX_ROUNDS="${MAX_ROUNDS:-25}"
SLEEP_SECS="${SLEEP_SECS:-20}"
RESET_DEEP="${RESET_DEEP:-1}"
DISPATCH_MAX_SECS="${DISPATCH_MAX_SECS:-5400}"
ALLOW_MODEL_REGEX="${ALLOW_MODEL_REGEX:-(gemma|qwen|r1|deepseek)}"
DENY_MODEL_REGEX="${DENY_MODEL_REGEX:-(cpu|tiny|mini)}"
MIN_MODEL_SIZE_B="${MIN_MODEL_SIZE_B:-14}"

MANIFEST="$REPO/reports/blueprint_audit/llm_results/fleet_dispatch_manifest.json"
DEEP_DIR="$REPO/reports/blueprint_audit/llm_results/deep"
LOG_DIR="$REPO/var/log"
RUN_LOG="$LOG_DIR/blueprint_audit_deep_watchdog.log"

mkdir -p "$DEEP_DIR" "$LOG_DIR"

log() {
  local msg="[$(date '+%Y-%m-%d %H:%M:%S')] $*"
  echo "$msg" | tee -a "$RUN_LOG"
}

kill_stale_dispatchers() {
  local pids
  pids=$(pgrep -f "$REPO/scripts/blueprint_audit_fleet_dispatch.py" || true)
  if [[ -n "$pids" ]]; then
    log "stopping stale dispatcher pids: $pids"
    kill $pids || true
    sleep 2
  fi
}

clear_deep_outputs() {
  if [[ "$RESET_DEEP" == "1" ]]; then
    log "clearing previous deep outputs"
    rm -f "$DEEP_DIR"/batch-*.json
    rm -f "$MANIFEST"
  fi
}

next_batch_ids() {
  python3 - "$REPO" "$MANIFEST" << 'PY'
import json, sys, pathlib
repo = pathlib.Path(sys.argv[1])
manifest_path = pathlib.Path(sys.argv[2])
all_batches = json.loads((repo / 'reports' / 'blueprint_audit' / 'llm_dispatch_batches.json').read_text(encoding='utf-8'))
all_ids = [str(b.get('batch_id')) for b in all_batches if b.get('batch_id')]
if not manifest_path.exists():
    print(' '.join(all_ids))
    raise SystemExit(0)
m = json.loads(manifest_path.read_text(encoding='utf-8'))
by_id = {str(b.get('batch_id')): b for b in m.get('batches', []) if b.get('batch_id')}
need = []
for bid in all_ids:
    rec = by_id.get(bid)
    if not rec:
        need.append(bid)
        continue
    st = str(rec.get('status', ''))
    processed = int(rec.get('processed_size') or 0)
    if st != 'ok' or processed <= 0:
        need.append(bid)
print(' '.join(need))
PY
}

run_round() {
  local ids=("$@")
  if [[ ${#ids[@]} -eq 0 ]]; then
    return 0
  fi

  if [[ -x "$REPO/scripts/fleet-route-health.sh" ]]; then
    "$REPO/scripts/fleet-route-health.sh" >> "$RUN_LOG" 2>&1 || log "route health probe reported failures; attempting dispatch anyway"
  fi

  log "dispatch round for batch ids: ${ids[*]}"
  set +e
  timeout --signal=TERM "$DISPATCH_MAX_SECS" \
    python3 "$REPO/scripts/blueprint_audit_fleet_dispatch.py" \
    --repo "$REPO" \
    --parallel "$PARALLEL" \
    --skip-done \
    --chunk-size "$CHUNK_SIZE" \
    --min-chunk-size "$MIN_CHUNK_SIZE" \
    --allow-model-regex "$ALLOW_MODEL_REGEX" \
    --deny-model-regex "$DENY_MODEL_REGEX" \
    --min-model-size-b "$MIN_MODEL_SIZE_B" \
    --batch-ids "${ids[@]}" \
    >> "$RUN_LOG" 2>&1
  local rc=$?
  set -e

  if [[ "$rc" -eq 124 ]]; then
    log "dispatch round timed out after ${DISPATCH_MAX_SECS}s; forcing stale dispatcher cleanup"
    kill_stale_dispatchers
    return 1
  fi
  if [[ "$rc" -ne 0 ]]; then
    log "dispatch exited with code $rc; continuing to next retry round"
    return 1
  fi
  return 0
}

main() {
  log "deep watchdog start: parallel=$PARALLEL chunk=$CHUNK_SIZE min_chunk=$MIN_CHUNK_SIZE max_dispatch=${DISPATCH_MAX_SECS}s"
  kill_stale_dispatchers
  clear_deep_outputs

  local round=1
  while [[ "$round" -le "$MAX_ROUNDS" ]]; do
    read -r -a ids <<< "$(next_batch_ids)"
    if [[ ${#ids[@]} -eq 0 ]]; then
      log "all batches complete"
      return 0
    fi

    log "round $round/$MAX_ROUNDS pending=${#ids[@]}"
    if ! run_round "${ids[@]}"; then
      log "round $round ended without completion; retrying pending batches"
    fi
    ((round++))
    sleep "$SLEEP_SECS"
  done

  log "max rounds reached; unresolved batches remain"
  return 1
}

main "$@"
