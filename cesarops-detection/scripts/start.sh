#!/usr/bin/env bash
# Start the cesarops-detection HTTP service on port 5580.
# Wires the right LAN IPs for the vision workers + degrades gracefully
# when the TPU jitter VM isn't up (2-lock mode).
set -e

# Vision workers — LAN IPs (cesarops2 = 10.0.0.129, cesarops3 = 10.0.0.41)
# Old code defaults pointed at Tailscale IPs; LAN is faster and more reliable.
export SCOUT_URL="${SCOUT_URL:-http://10.0.0.41:5570}"          # cesarops3 — Florence-2 on 1060
export VALIDATOR_URL="${VALIDATOR_URL:-http://10.0.0.129:5571}" # cesarops2 — Moondream2 on P1000
export JITTER_URL="${JITTER_URL:-http://192.168.122.10:8080}"   # TPU VM — degrade gracefully if down
export DETECTION_PORT="${DETECTION_PORT:-5580}"

# Build if missing (one-time)
SCRIPT_DIR="$( cd "$(dirname "${BASH_SOURCE[0]}")" && pwd )"
WORKSPACE_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BIN="$WORKSPACE_ROOT/target/release/cesarops-detection"

if [[ ! -x "$BIN" ]]; then
    echo "[start.sh] cesarops-detection binary not found, building..."
    cd "$WORKSPACE_ROOT"
    cargo build --release -p cesarops-detection
fi

echo "[start.sh] Starting cesarops-detection on :$DETECTION_PORT"
echo "[start.sh]   SCOUT     $SCOUT_URL"
echo "[start.sh]   VALIDATOR $VALIDATOR_URL"
echo "[start.sh]   JITTER    $JITTER_URL  (optional; degrades to 2-lock if offline)"

exec "$BIN"
