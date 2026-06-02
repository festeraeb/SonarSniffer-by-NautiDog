#!/usr/bin/env bash
# NOT IN FLEET PLAN — WiFi penalty too high for 72B sharding. Kobold :5571 only.
# Optional reference if Nautik9 is on wired LAN: see topology_fleet_hetero_70b_with_laptop.yml.example
set -euo pipefail

CAKE="${CAKE:-/opt/cesarops/cake/bin/cake}"
CAKE_MODEL="${CAKE_MODEL:-Qwen/Qwen2.5-72B-Instruct}"
PORT="${CAKE_WORKER_PORT:-10133}"
NAME="${CAKE_WORKER_NAME:-nautik-m2200}"
# Maxwell M2200 — build with: CUDA_COMPUTE_CAP=52 bash scripts/install_cake_fleet.sh

if ! command -v "$CAKE" >/dev/null 2>&1 && [[ ! -x "$CAKE" ]]; then
  echo "[laptop-cake] cake not found — install cake-cli with CUDA on the laptop first"
  exit 1
fi

pkill -f "cake run.*${PORT}" 2>/dev/null || true
sleep 1

echo "[laptop-cake] starting worker $NAME on 0.0.0.0:${PORT} (M2200 device 0)"
# NOTE: current Cake CLI uses `cake run` with no model + --cluster-key to start as a worker.
# If you want standalone local inference on the laptop, run `cake run $CAKE_MODEL` instead.
setsid "$CAKE" run --device 0 --name "$NAME" --address "0.0.0.0:${PORT}" --cluster-key "${CAKE_CLUSTER_KEY:-}" \
  >>"${HOME}/cake-m2200-worker.log" 2>&1 &
echo $! >"${HOME}/cake-m2200-worker.pid"
echo "[laptop-cake] pid=$(cat "${HOME}/cake-m2200-worker.pid") log=~/cake-m2200-worker.log"
echo "[laptop-cake] T440 master will connect to http://100.110.214.86:${PORT}"
