#!/usr/bin/env bash
# Start cesarops-detection on T440 with resilient worker routing.
#
# Tries cesarops2 (10.0.0.201) vision workers first; falls back to local CPU sim.
# If no scout is reachable, starts local cpu_sim_workers automatically.
#
# cesarops2 lab (run ON cesarops2): scripts/cesarops2_research_lab.sh
set -euo pipefail

CESAROPS2_IPS="${CESAROPS2_IPS:-10.0.0.201 10.0.0.200}"
REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
WORKSPACE_ROOT="${WORKSPACE_ROOT:-/codebase/repos/wreckhunter2000-1}"
DETECTION_PORT="${DETECTION_PORT:-5580}"
SIM_PY="${REPO}/cesarops-detection/workers/cpu_sim_workers.py"
VENV="${VENV:-/tmp/venv-detection-t440}"
PID_DIR="${PID_DIR:-/tmp/cesarops-detection-lab}"
mkdir -p "$PID_DIR"

probe() {
  curl -sf --max-time 3 "${1%/}/health" >/dev/null 2>&1
}

pick_first() {
  for url in "$@"; do
    if probe "$url"; then
      echo "$url"
      return 0
    fi
  done
  return 1
}

ensure_local_cpu_sim() {
  if probe "http://127.0.0.1:5570" && probe "http://127.0.0.1:5572" && probe "http://127.0.0.1:8180"; then
    echo "[start.sh] Local CPU sim workers already up"
    return 0
  fi
  if [[ ! -f "$SIM_PY" ]]; then
    echo "[start.sh] WARN: $SIM_PY missing — cannot start local fallback"
    return 1
  fi
  if [[ ! -x "$VENV/bin/python" ]]; then
    python3 -m venv "$VENV"
    "$VENV/bin/python" -m pip install -q fastapi uvicorn pillow numpy
  fi
  pkill -f "cpu_sim_workers.py" 2>/dev/null || true
  sleep 1
  for role in scout validator jitter; do
    setsid "$VENV/bin/python" "$SIM_PY" "$role" --host 127.0.0.1 \
      >>"$PID_DIR/cpu-${role}.log" 2>&1 &
    disown 2>/dev/null || true
  done
  sleep 2
  echo "[start.sh] Started local CPU sim workers (T440 fallback)"
}

# Build URL pools from both cesarops2 NICs (eno1=.200, eno2=.201)
_scout_urls=()
_val_urls=()
_jit_urls=()
for ip in $CESAROPS2_IPS; do
  _scout_urls+=("http://${ip}:5570")
  _val_urls+=("http://${ip}:5572" "http://${ip}:5571")
  _jit_urls+=("http://${ip}:8080")
done
_scout_urls+=("http://127.0.0.1:5570")
_val_urls+=("http://127.0.0.1:5572")
_jit_urls+=("http://10.0.0.61:8180" "http://10.0.0.61:8080")
_jit_urls+=("http://127.0.0.1:8180")

export SCOUT_URLS="${SCOUT_URLS:-$(IFS=,; echo "${_scout_urls[*]}")}"
export VALIDATOR_URLS="${VALIDATOR_URLS:-$(IFS=,; echo "${_val_urls[*]}")}"
export JITTER_URLS="${JITTER_URLS:-$(IFS=,; echo "${_jit_urls[*]}")}"

if ! pick_first ${_scout_urls[@]} >/dev/null 2>&1; then
  ensure_local_cpu_sim || true
fi

BIN="$WORKSPACE_ROOT/target/release/cesarops-detection"
if [[ ! -x "$BIN" ]]; then
  echo "[start.sh] Building cesarops-detection..."
  (cd "$WORKSPACE_ROOT" && cargo build --release -p cesarops-detection)
fi

pkill -x cesarops-detection 2>/dev/null || pkill -f "/target/release/cesarops-detection" 2>/dev/null || true
fuser -k "${DETECTION_PORT}/tcp" 2>/dev/null || true
sleep 1

echo "[start.sh] SCOUT_URLS=$SCOUT_URLS"
echo "[start.sh] VALIDATOR_URLS=$VALIDATOR_URLS"
echo "[start.sh] JITTER_URLS=$JITTER_URLS"

nohup env SCOUT_URLS="$SCOUT_URLS" VALIDATOR_URLS="$VALIDATOR_URLS" JITTER_URLS="$JITTER_URLS" \
  DETECTION_PORT="$DETECTION_PORT" \
  "$BIN" > /tmp/cesarops-detection.log 2>&1 &

sleep 2
curl -sf "http://127.0.0.1:${DETECTION_PORT}/health" | head -c 400 || {
  echo "[start.sh] detection failed to start — see /tmp/cesarops-detection.log"
  tail -20 /tmp/cesarops-detection.log
  exit 1
}
echo
echo "[start.sh] cesarops-detection on :${DETECTION_PORT} (failover pools active)"
