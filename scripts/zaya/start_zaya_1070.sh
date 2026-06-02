#!/usr/bin/env bash
# ZAYA1-8B thinker on GTX 1070 — fleet preset dual-coder-zaya :5203
set -euo pipefail

PORT="${PORT:-5203}"
HOST="${HOST:-0.0.0.0}"
# llama.cpp-zaya exposes 1070 as Vulkan2 (not CUDA2)
DEVICE="${DEVICE:-Vulkan2}"
CTX="${CTX:-4096}"
NGL="${NGL:-99}"
MODELS="${MODELS:-/mnt/t440/models}"
LOG="${LOG:-/data/cesarops/logs/zaya-1070-${PORT}.log}"
LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp-zaya/build-vk/bin/llama-server}"

mkdir -p "$(dirname "$LOG")"

if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
  echo "[zaya-1070] already up :${PORT}"
  exit 0
fi

pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
fuser -k "${PORT}/tcp" 2>/dev/null || true
sleep 1

MODEL=""
for m in \
  "${MODELS}/ZAYA1-8B-Q4_K_M.gguf" \
  "${MODELS}/ZAYA1-8B-Q8_0.gguf" \
  /data/cesarops/models/ZAYA1-8B-Q4_K_M.gguf \
  /home/cesarops/models/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q4_K_M.gguf \
  /home/cesarops/models/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q8_0.gguf; do
  [[ -f "$m" ]] && MODEL="$m" && break
done
if [[ -z "$MODEL" ]]; then
  echo "[zaya-1070] no ZAYA1-8B GGUF under ${MODELS}" >&2
  exit 1
fi

nohup "$LLAMA" -m "$MODEL" --host "$HOST" --port "$PORT" -dev "$DEVICE" \
  -ngl "$NGL" -fa auto -ctk q8_0 -ctv q8_0 -ub 384 -c "$CTX" -t 4 -np 1 \
  --reasoning off --timeout 600 >>"$LOG" 2>&1 &

for _ in $(seq 1 48); do
  if curl -sf --max-time 3 "http://127.0.0.1:${PORT}/v1/models" >/dev/null; then
    echo "[zaya-1070] ready http://127.0.0.1:${PORT} model=$(basename "$MODEL") dev=$DEVICE"
    exit 0
  fi
  sleep 5
done
echo "[zaya-1070] failed — tail $LOG" >&2
exit 1
