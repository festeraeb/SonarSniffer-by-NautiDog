#!/usr/bin/env bash
# Enhance 26 B-grade integrate modules via Qwen3.6-35B-A3B (:5200) + Gemma-4 (:5571).
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
LOG="${LOG:-/tmp/bgrade-enhance-dispatch.log}"
PIDFILE="${PIDFILE:-/tmp/bgrade-enhance-dispatch.pid}"

cd "$REPO/scripts/integrate"

case "${1:-start}" in
  start)
    pkill -f dispatch_bgrade_enhance.py 2>/dev/null || true
    pkill -f dispatch_forge_rust_port.py 2>/dev/null || true
    : >"$LOG"
    nohup env REPO="$REPO" LOG="$LOG" \
      python3 -u dispatch_bgrade_enhance.py >>"$LOG" 2>&1 &
    echo $! >"$PIDFILE"
    echo "bgrade enhance started pid=$(cat "$PIDFILE") log=$LOG"
    ;;
  status)
    if [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
      echo "running pid=$(cat "$PIDFILE")"
    else
      echo "not running"
    fi
    tail -20 "$LOG" 2>/dev/null || true
    ;;
  stop)
    kill "$(cat "$PIDFILE")" 2>/dev/null || true
    pkill -f dispatch_bgrade_enhance.py 2>/dev/null || true
    rm -f "$PIDFILE"
    echo "stopped"
    ;;
  *)
    echo "usage: $0 {start|stop|status}"
    exit 1
    ;;
esac
