#!/usr/bin/env bash
# ML350e — Coral jitter validator (:8190) per CORAL_TPU_WORKER_SPEC.md
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
[[ -d "$REPO/cesarops-detection/jitter-rs" ]] || REPO="/codebase/repos/wreckhunter2000-1"

WORKER="$REPO/cesarops-detection/jitter-rs/coral_jitter_worker.py"
VENV="${CORAL_VENV:-${HOME}/.venvs/coral-jitter}"
PORT="${CORAL_PORT:-8190}"
PID_DIR="${PID_DIR:-${HOME}/.cache/cesarops/coral-jitter}"
LOG="${CORAL_LOG:-${HOME}/.cache/cesarops/coral-jitter.log}"

export CORAL_PORT="${CORAL_PORT:-8190}"
export CORAL_MODEL="${CORAL_MODEL:-/opt/cesarops/models/jitter_edgetpu.tflite}"
export CORAL_OUTPUT="${CORAL_OUTPUT:-auto}"
# export FLEET_KEY=...   # optional

log() { echo "[coral-jitter] $*"; }

if ls /dev/apex_* >/dev/null 2>&1; then
  log "apex: $(ls /dev/apex_* | tr '\n' ' ')"
else
  log "WARN: no /dev/apex_* — worker runs stub_rule or CPU tflite"
fi

mkdir -p "$PID_DIR" "$(dirname "$LOG")"
if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install -U pip wheel
  "$VENV/bin/pip" install numpy
  "$VENV/bin/pip" install tflite-runtime 2>/dev/null || true
  "$VENV/bin/pip" install pycoral 2>/dev/null || true
fi

pkill -f "coral_jitter_worker.py.*${PORT}" 2>/dev/null || true
fuser -k "${PORT}/tcp" 2>/dev/null || true
sleep 1

setsid "$VENV/bin/python" "$WORKER" >>"$LOG" 2>&1 &
echo $! >"$PID_DIR/coral-jitter.pid"
log "started pid=$(cat "$PID_DIR/coral-jitter.pid") port=$PORT log=$LOG"

for _ in $(seq 1 25); do
  if curl -sf "http://127.0.0.1:${PORT}/health" >/dev/null; then
    curl -s "http://127.0.0.1:${PORT}/health" | python3 -m json.tool
    exit 0
  fi
  sleep 0.4
done
log "health check failed — see $LOG"
exit 1
