#!/usr/bin/env bash
# RS LLM summarize when Forge is idle (RTX thinker / Gemma E4B).
#
#   bash scripts/fleet_rs_llm_summarize.sh start    # background watch loop
#   bash scripts/fleet_rs_llm_summarize.sh monitor  # tail progress
#   bash scripts/fleet_rs_llm_summarize.sh status
#   bash scripts/fleet_rs_llm_summarize.sh stop
#
set -euo pipefail

REPO="${REPO:-/data/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/scripts" ]] || REPO="/mnt/t440/codebase/repos/wreckhunter2000-1"
export REPO
export FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
export THINKER_URL="${THINKER_URL:-http://127.0.0.1:5200}"
export FLEET_CATALOG_DIR="${FLEET_CATALOG_DIR:-$REPO/var/fleet-catalog}"

PY="${PYTHON:-python3}"
SCRIPT="$REPO/scripts/fleet_rs_llm_summarize.py"
PID_FILE="${RS_SUMMARIZE_PID:-$FLEET_CATALOG_DIR/rs_llm_summarize.pid}"
LOG="${RS_SUMMARIZE_LOG:-$FLEET_CATALOG_DIR/rs_llm_summarize.log}"
POLL_SEC="${RS_SUMMARIZE_POLL_SEC:-30}"
BATCH_SIZE="${RS_SUMMARIZE_BATCH_SIZE:-3}"

log() { echo "[rs-summarize] $*"; }

cmd_start() {
  mkdir -p "$FLEET_CATALOG_DIR"
  if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    log "already running pid=$(cat "$PID_FILE")"
    exit 0
  fi
  if ! curl -sf --max-time 3 "${THINKER_URL}/v1/models" >/dev/null; then
    log "warn: thinker ${THINKER_URL} not up — start: bash scripts/cesarops2_fleet_roles.sh start"
  fi
  # Fresh progress snapshot (summaries jsonl is append-only across restarts).
  "$PY" -c "
import json, time
from pathlib import Path
p = Path('$FLEET_CATALOG_DIR/rs_llm_summarize_progress.json')
done = sum(1 for _ in Path('$FLEET_CATALOG_DIR/rs_llm_summaries.jsonl').open() if _.strip()) if Path('$FLEET_CATALOG_DIR/rs_llm_summaries.jsonl').is_file() else 0
p.write_text(json.dumps({'started_at': int(time.time()), 'done': done, 'errors': 0, 'status': 'starting'}, indent=2))
"
  nohup "$PY" "$SCRIPT" --watch --poll-sec "$POLL_SEC" --batch-size "$BATCH_SIZE" \
    >>"$LOG" 2>&1 &
  echo $! >"$PID_FILE"
  log "started pid=$(cat "$PID_FILE") log=$LOG"
  log "monitor: bash scripts/fleet_rs_llm_summarize.sh monitor"
}

cmd_stop() {
  if [[ -f "$PID_FILE" ]]; then
    kill "$(cat "$PID_FILE")" 2>/dev/null || true
    rm -f "$PID_FILE"
    log "stopped"
  else
    log "not running"
  fi
}

cmd_monitor() {
  touch "$LOG"
  echo "=== progress ==="
  if [[ -f "$FLEET_CATALOG_DIR/rs_llm_summarize_progress.json" ]]; then
    cat "$FLEET_CATALOG_DIR/rs_llm_summarize_progress.json"
  fi
  echo "=== log (tail) ==="
  tail -n 40 -f "$LOG"
}

cmd_status() {
  "$PY" "$SCRIPT" --status
  echo "---"
  wc -l "$FLEET_CATALOG_DIR/rs_llm_summaries.jsonl" 2>/dev/null || echo "no summaries yet"
  [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null && echo "runner pid=$(cat "$PID_FILE") alive" || echo "runner not active"
}

case "${1:-status}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  monitor) cmd_monitor ;;
  status) cmd_status ;;
  *)
    echo "Usage: $0 {start|stop|monitor|status}"
    exit 1
    ;;
esac
