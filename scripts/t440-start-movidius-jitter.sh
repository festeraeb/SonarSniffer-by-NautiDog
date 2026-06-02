#!/usr/bin/env bash
# DEPRECATED — use scripts/t440-start-jitter-rs.sh (Nomad t440-movidius-jitter job).
# Legacy Python path; OpenVINO 2026+ has no MYRIAD — falls back to cpu_thermal silently.
set -euo pipefail
echo "[t440-jitter] DEPRECATED: use t440-start-jitter-rs.sh" >&2

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
VISION_DIR="$REPO/cesarops-detection/vision-workers"
VENV="${VENV:-/home/cesarops/.venvs/cesarops-lab}"
PID_DIR="${PID_DIR:-/tmp/cesarops-t440-jitter}"
PORT="${JITTER_PORT:-8180}"

mkdir -p "$PID_DIR"
log() { echo "[t440-jitter] $*"; }

if ! lsusb 2>/dev/null | grep -q '03e7:'; then
  log "WARN: no Movidius (03e7) on USB — starting CPU thermal fallback anyway"
fi

if [[ ! -x "$VENV/bin/python" ]]; then
  python3 -m venv "$VENV"
  "$VENV/bin/pip" install -q fastapi uvicorn pillow numpy
fi
# OpenVINO + NCS plugin (optional; falls back to CPU heuristic)
"$VENV/bin/pip" install -q openvino 2>/dev/null || true

pkill -f "jitter_movidius.py.*${PORT}" 2>/dev/null || true
fuser -k "${PORT}/tcp" 2>/dev/null || true
sleep 1

export JITTER_PORT="$PORT"
export OPENVINO_DEVICE="${OPENVINO_DEVICE:-MYRIAD}"
export JITTER_MODEL="${JITTER_MODEL:-}"

log "starting jitter_movidius port=$PORT device=$OPENVINO_DEVICE (foreground for Nomad)"
# Nomad service tasks must not exit — run uvicorn in foreground.
exec "$VENV/bin/python" "$VISION_DIR/jitter_movidius.py"
