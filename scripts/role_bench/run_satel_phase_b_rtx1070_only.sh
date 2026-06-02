#!/usr/bin/env bash
# Phase B only: RTX :5200 + GTX 1070 :5202 coder models (satellite wreck prompt).
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/role_bench/_satel_phase_b.py" ]] || REPO="/data/codebase/repos/wreckhunter2000-1"
SRC="${1:-$REPO/var/role_bench/satel_glint_20260601T154030Z}"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
OUT="${OUT:-$REPO/var/role_bench/satel_glint_${STAMP}_rtx1070}"
mkdir -p "$OUT"
cp -f "$SRC/accel_scan_packet.json" "$OUT/"

# Bench ports off the mission-watchdog triple stack (5200/5201/5202).
PORT_RTX="${PORT_RTX:-5210}"
PORT_GTX="${PORT_GTX:-5212}"
export RTX_CODER_URL="http://127.0.0.1:${PORT_RTX}"
export GTX_CODER_URL="http://127.0.0.1:${PORT_GTX}"
export PHASE_B_MAX_TOKENS="${PHASE_B_MAX_TOKENS:-2048}"

LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
M="${MODELS:-/mnt/t440/models}"
PID_DIR=/tmp/cesarops2-role-bench-coders
mkdir -p "$PID_DIR"

stop_port() {
  local port=$1
  pkill -f "llama-server.*--port ${port}" 2>/dev/null || true
  fuser -k "${port}/tcp" 2>/dev/null || true
}
stop_port "$PORT_RTX"
stop_port "$PORT_GTX"
# Mission watchdog stack uses :5200/:5201/:5202 — must stop all or CUDA0/1 VRAM stays taken.
for p in 5200 5201 5202 5571; do
  pkill -f "llama-server.*--port ${p}" 2>/dev/null || true
  fuser -k "${p}/tcp" 2>/dev/null || true
done
sleep 3

# Load 1070 first (needs ~7GB); then 2060 14B partial-GPU.
setsid "$LLAMA" -m "$M/Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf" --host 0.0.0.0 --port "$PORT_GTX" \
  -dev CUDA1 -ngl 99 -fa auto -ctk q8_0 -ctv q8_0 -ub 384 -c 4096 -t 4 -np 1 \
  --reasoning off --timeout 900 </dev/null >>"$PID_DIR/llama-${PORT_GTX}.log" 2>&1 &

for i in $(seq 1 48); do
  curl -sf --max-time 5 "${GTX_CODER_URL}/v1/models" >/dev/null && break
  sleep 5
done

setsid "$LLAMA" -m "$M/qwen2.5-coder-14b-instruct-q4_k_m.gguf" --host 0.0.0.0 --port "$PORT_RTX" \
  -dev CUDA0 -ngl 34 -sm layer -fa auto -ctk q8_0 -ctv q8_0 -ub 256 -c 6144 -t 4 -np 1 \
  --reasoning off --timeout 900 </dev/null >>"$PID_DIR/llama-${PORT_RTX}.log" 2>&1 &

for i in $(seq 1 60); do
  curl -sf --max-time 5 "${RTX_CODER_URL}/v1/models" >/dev/null && ok0=1 || ok0=0
  curl -sf --max-time 5 "${GTX_CODER_URL}/v1/models" >/dev/null && ok2=1 || ok2=0
  [[ "$ok0" == 1 && "$ok2" == 1 ]] && break
  sleep 5
done

{
  echo "# models at run time (bench ports ${PORT_RTX}/${PORT_GTX})"
  curl -sf "${RTX_CODER_URL}/v1/models" | python3 -c "import json,sys; d=json.load(sys.stdin); print('${PORT_RTX}', d['data'][0]['id'])"
  curl -sf "${GTX_CODER_URL}/v1/models" | python3 -c "import json,sys; d=json.load(sys.stdin); print('${PORT_GTX}', d['data'][0]['id'])"
  nvidia-smi --query-gpu=index,name,memory.used --format=csv
} | tee "$OUT/MODELS_AT_RUN.txt"

CODERS_ONLY=RTX1070 python3 "$REPO/scripts/role_bench/_satel_phase_b.py" "$OUT"
echo "OUT=$OUT"
