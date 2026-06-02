#!/usr/bin/env bash
# Start hetero 70B fleet, live TFLOPS monitor, fire inference (streamed when supported).
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CAKE_API="${CAKE_API:-http://127.0.0.1:8081}"
CAKE_LOG="${CAKE_LOG:-$HOME/.cache/cesarops/cake_fleet.log}"
MONITOR_LOG="${MONITOR_LOG:-/tmp/cake-70b-monitor.log}"
TASK_LOG="${TASK_LOG:-/tmp/cake-70b-task.log}"
WAIT_SECS="${WAIT_SECS:-3600}"
MAX_TOKENS="${MAX_TOKENS:-2048}"
USE_INFERENCE_FIX="${USE_INFERENCE_FIX:-0}"

log() { echo "[70b-live] $*" | tee -a "$CAKE_LOG" "$TASK_LOG"; }

: >"$TASK_LOG"
log "starting hetero fleet…"
WAIT_WORKERS="${WAIT_WORKERS:-60}" bash "${SCRIPT_DIR}/start-fleet-hetero-70b.sh" >>"$TASK_LOG" 2>&1 || true

if ! pgrep -f "monitor-70b-live.sh" >/dev/null 2>&1; then
  log "starting GPU/log monitor → $MONITOR_LOG"
  setsid bash "${SCRIPT_DIR}/monitor-70b-live.sh" >>"${MONITOR_LOG}.stdout" 2>&1 &
  echo $! > /tmp/cake-70b-monitor.pid
fi

log "waiting for ${CAKE_API}/v1/models (max ${WAIT_SECS}s)…"
deadline=$((SECONDS + WAIT_SECS))
until curl -sf --max-time 8 "${CAKE_API}/v1/models" >/dev/null 2>&1; do
  if (( SECONDS >= deadline )); then
    log "timeout — tail $CAKE_LOG $MONITOR_LOG"
    exit 1
  fi
  sleep 10
done
log "Cake API online"

if [[ "$USE_INFERENCE_FIX" == "1" ]]; then
  log "running full inference-engine fix prompt…"
  exec bash "${SCRIPT_DIR}/run-qwen72b-inference-fix.sh"
fi

PROMPT="${PROMPT:-Summarize in 3 bullets what a heterogeneous Cake 72B fleet is doing right now on this cluster (T440 P100, cesarops2 P106/2060/1070, CPU RAM). Be concrete.}"
log "POST ${CAKE_API}/v1/chat/completions stream=true max_tokens=${MAX_TOKENS}"

{
  echo "=== $(date -u -Iseconds) request ==="
  curl -sfN --max-time 7200 \
    -X POST "${CAKE_API}/v1/chat/completions" \
    -H 'Content-Type: application/json' \
    -d "$(python3 -c "
import json, os
print(json.dumps({
  'model': 'Qwen/Qwen2.5-72B-Instruct',
  'stream': True,
  'max_tokens': int(os.environ.get('MAX_TOKENS', '2048')),
  'temperature': 0.3,
  'messages': [{'role': 'user', 'content': os.environ.get('PROMPT', '')}],
}))
" )" 2>&1 | while IFS= read -r line; do
    echo "$line" | tee -a "$TASK_LOG"
    if [[ "$line" == data:* ]]; then
      chunk="${line#data: }"
      [[ "$chunk" == "[DONE]" ]] && continue
      python3 -c "
import json,sys
try:
  d=json.loads(sys.argv[1])
  t=d.get('choices',[{}])[0].get('delta',{}).get('content','')
  if t: print(t, end='', flush=True)
except Exception: pass
" "$chunk" 2>/dev/null | tee -a "${TASK_LOG}.stream"
    fi
  done
  echo ""
  echo "=== $(date -u -Iseconds) done ==="
} | tee -a "$TASK_LOG"

log "task log: $TASK_LOG | stream: ${TASK_LOG}.stream | monitor: $MONITOR_LOG"
