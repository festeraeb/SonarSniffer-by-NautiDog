#!/usr/bin/env bash
# Start triple-lock vision workers (scout / validator / jitter).
#
# Modes:
#   cpu   — numpy sim (default, no GPU)
#   gpu   — Florence-2 scout + Moondream2 validator
#   yolo  — YOLO11 scout + Moondream validator (CPU) + Movidius jitter stub
#
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
VISION_DIR="$REPO/cesarops-detection/vision-workers"
SIM_PY="$REPO/cesarops-detection/workers/cpu_sim_workers.py"
PID_DIR="${PID_DIR:-/tmp/cesarops-vision}"
VENV="${VENV:-$HOME/.venvs/cesarops-lab}"
VISION_MODE="${VISION_MODE:-cpu}"
VISION_MODEL_ROOT="${VISION_MODEL_ROOT:-/data/cesarops/vision_models}"
HOST="${VISION_HOST:-127.0.0.1}"

mkdir -p "$PID_DIR"
log() { echo "[vision] $*"; }

ensure_venv() {
  if [[ ! -x "$VENV/bin/python" ]]; then
    python3 -m venv "$VENV"
    "$VENV/bin/pip" install -q fastapi uvicorn pillow numpy
  fi
  case "$VISION_MODE" in
    gpu) "$VENV/bin/pip" install -q torch 'transformers==4.46.3' 'tokenizers<0.21' einops timm 2>/dev/null || true ;;
    yolo) "$VENV/bin/pip" install -q ultralytics 2>/dev/null || true ;;
  esac
  PYTHON="$VENV/bin/python"
}

stop_all() {
  pkill -f "scout_1060.py|scout_yolo11.py|validator_p1000.py|jitter_movidius.py|cpu_sim_workers.py" 2>/dev/null || true
  for port in 5570 5572 8080; do fuser -k "${port}/tcp" 2>/dev/null || true; done
}

probe() {
  curl -sf --max-time 3 "$1" >/dev/null && log "  OK $2" || log "  -- $2 (not ready)"
}

cmd_start() {
  stop_all
  ensure_venv
  case "$VISION_MODE" in
    gpu)
      CUDA_VISIBLE_DEVICES="${SCOUT_CUDA_VISIBLE:-0}" \
      VISION_MODEL_ROOT="$VISION_MODEL_ROOT" SCOUT_PORT=5570 SCOUT_CUDA_DEVICE=0 \
        setsid "$PYTHON" "$VISION_DIR/scout_1060.py" >>"$PID_DIR/scout.log" 2>&1 &
      VISION_MODEL_ROOT="$VISION_MODEL_ROOT" VALIDATOR_PORT=5572 \
        setsid "$PYTHON" "$VISION_DIR/validator_p1000.py" >>"$PID_DIR/validator.log" 2>&1 &
      JITTER_PORT=8080 setsid "$PYTHON" "$VISION_DIR/jitter_movidius.py" >>"$PID_DIR/jitter.log" 2>&1 &
      ;;
    yolo)
      SCOUT_PORT=5570 YOLO_MODEL="${YOLO_MODEL:-yolo11n.pt}" DEVICE="${YOLO_DEVICE:-cpu}" \
        setsid "$PYTHON" "$VISION_DIR/scout_yolo11.py" >>"$PID_DIR/scout.log" 2>&1 &
      VISION_MODEL_ROOT="$VISION_MODEL_ROOT" VALIDATOR_PORT=5572 DEVICE=cpu \
        setsid "$PYTHON" "$VISION_DIR/validator_p1000.py" >>"$PID_DIR/validator.log" 2>&1 &
      JITTER_PORT=8080 setsid "$PYTHON" "$VISION_DIR/jitter_movidius.py" >>"$PID_DIR/jitter.log" 2>&1 &
      ;;
    *)
      for role in scout validator jitter; do
        if [[ "$role" == "jitter" ]]; then
          JITTER_PORT="${JITTER_PORT:-8180}" setsid "$PYTHON" "$SIM_PY" "$role" --host "$HOST" >>"$PID_DIR/cpu-${role}.log" 2>&1 &
        else
          setsid "$PYTHON" "$SIM_PY" "$role" --host "$HOST" >>"$PID_DIR/cpu-${role}.log" 2>&1 &
        fi
      done
      ;;
  esac
  log "started mode=$VISION_MODE host=$HOST"
  sleep 2
  cmd_status
}

cmd_status() {
  probe "http://${HOST}:5570/health" "scout:5570"
  probe "http://${HOST}:5572/health" "validator:5572"
  probe "http://${HOST}:${JITTER_PORT:-8180}/health" "jitter"
}

cmd_stop() { stop_all; log "stopped"; }

chmod +x "$0" 2>/dev/null || true
case "${1:-start}" in
  start) cmd_start ;;
  stop) cmd_stop ;;
  status) cmd_status ;;
  *) echo "Usage: $0 {start|stop|status}"; exit 1 ;;
esac
