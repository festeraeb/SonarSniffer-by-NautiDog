#!/usr/bin/env bash
# Push the just-built cesarops-inference binary from T440 to cesarops2 (1070).
# Uses a one-shot HTTP server (tailscale file cp needs sudo on this box).
# Idempotent — safe to re-run.
set -eu

REMOTE_HOST="cesarops@cesarops2"
T440_LAN_IP="${T440_LAN_IP:-10.0.0.61}"
HTTP_PORT="${HTTP_PORT:-19987}"
SRC_BIN="/home/cesarops/wreckhunter2000-1/cesarops-inference/target/release/cesarops-inference"
DST_BIN="/home/cesarops/cesarops-engine/cesarops-inference"

[[ -x "$SRC_BIN" ]] || { echo "FAIL: $SRC_BIN not built. Run: cargo build --release"; exit 1; }

# Spawn an http.server bound to LAN, kill on exit
SRV_DIR="$(dirname "$SRC_BIN")"
python3 -m http.server "$HTTP_PORT" --bind "$T440_LAN_IP" --directory "$SRV_DIR" \
    > /tmp/push_engine_http.log 2>&1 &
HTTP_PID=$!
trap 'kill $HTTP_PID 2>/dev/null || true' EXIT
sleep 1

# Pull on remote
tailscale ssh "$REMOTE_HOST" "
    mkdir -p $(dirname "$DST_BIN")
    curl -fsS -o $DST_BIN http://${T440_LAN_IP}:${HTTP_PORT}/cesarops-inference
    chmod +x $DST_BIN
    ls -lh $DST_BIN
"
echo "PUSH OK"
