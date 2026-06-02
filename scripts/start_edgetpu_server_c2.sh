#!/usr/bin/env bash
# cesarops2 — Coral Edge TPU inference gateway (:8092). Uses real TPU when /dev/apex_* exists; else CPU stub.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
PORT="${TPU_PORT:-8092}"
PID_DIR="${PID_DIR:-/tmp/cesarops-tpu}"
VENV="${VENV:-/home/cesarops/.venvs/cesarops-tpu}"
SRC="${TPU_SERVER_PY:-$REPO/backup/deploy/tools/cesarops-core-github/tpu_server.py}"

mkdir -p "$PID_DIR"
log() { echo "[tpu-c2] $*"; }

if lspci -nn 2>/dev/null | grep -q '1ac1:089a'; then
  log "PCI Coral Edge TPU detected (07:00.0)"
fi
if ls /dev/apex_* >/dev/null 2>&1; then
  log "apex device nodes: $(ls /dev/apex_* | tr '\n' ' ')"
else
  log "WARN: no /dev/apex_* — gasket driver not loaded; inference uses CPU stub"
  log "      fix: build gasket-dkms for kernel $(uname -r) or use USB Accelerator"
fi

[[ -f "$SRC" ]] || { log "missing $SRC"; exit 1; }
if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install -q flask pillow numpy
  "$VENV/bin/pip" install -q tflite-runtime 2>/dev/null || true
  "$VENV/bin/pip" install -q pycoral 2>/dev/null || true
fi

pkill -f "tpu_server.py.*${PORT}" 2>/dev/null || true
fuser -k "${PORT}/tcp" 2>/dev/null || true
sleep 1

setsid "$VENV/bin/python" "$SRC" --host 0.0.0.0 --port "$PORT" >>"$PID_DIR/tpu.log" 2>&1 &
echo $! >"$PID_DIR/tpu.pid"
log "started pid=$(cat "$PID_DIR/tpu.pid") log=$PID_DIR/tpu.log"

for _ in $(seq 1 20); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/health" >/dev/null; then
    curl -sf "http://127.0.0.1:${PORT}/health"
    echo
    log "OK http://10.0.0.201:${PORT}/infer"
    exit 0
  fi
  sleep 1
done
tail -15 "$PID_DIR/tpu.log"
exit 1
