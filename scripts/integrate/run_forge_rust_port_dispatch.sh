#!/usr/bin/env bash
# Dispatch 51-module Rust port queue through Forge (T440) → cesarops2 :5200 / :5571
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://10.0.0.61:9100}"
LOG="${LOG:-/tmp/forge-rust-port-dispatch.log}"
PIDFILE="${PIDFILE:-/tmp/forge-rust-port-dispatch.pid}"

cd "$REPO/scripts/integrate"

case "${1:-start}" in
  start)
    if [[ -f "$PIDFILE" ]] && kill -0 "$(cat "$PIDFILE")" 2>/dev/null; then
      echo "already running pid=$(cat "$PIDFILE")"
      exit 0
    fi
    pkill -f dispatch_c2_rust_port.py 2>/dev/null || true
    : >"$LOG"
    nohup env FORGE_URL="$FORGE_URL" REPO="$REPO" LOG="$LOG" \
      python3 -u dispatch_forge_rust_port.py >/dev/null 2>&1 &
    echo $! >"$PIDFILE"
    echo "started pid=$(cat "$PIDFILE") log=$LOG"
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
    if [[ -f "$PIDFILE" ]]; then
      kill "$(cat "$PIDFILE")" 2>/dev/null || true
      rm -f "$PIDFILE"
    fi
    pkill -f dispatch_forge_rust_port.py 2>/dev/null || true
    echo "stopped"
    ;;
  *)
    echo "usage: $0 {start|status|stop}"
    exit 1
    ;;
esac
