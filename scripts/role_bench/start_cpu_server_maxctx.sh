#!/usr/bin/env bash
# Start llama-server CPU-only with context sized to available RAM (--fit).
set -euo pipefail

MODEL="${1:?model path}"
PORT="${2:?port}"
THREADS="${3:-12}"
LOG="${4:-/tmp/llama_server_${PORT}.log}"

LLAMA="${LLAMA:-/home/cesarops/src/llama.cpp/build/bin/llama-server}"
AVAIL_KB=$(awk '/^MemAvailable:/{print $2}' /proc/meminfo)
AVAIL_MB=$((AVAIL_KB / 1024))

# Rough weight footprint (MiB); override with MODEL_MB=...
MODEL_MB="${MODEL_MB:-24000}"
HEADROOM_MB="${HEADROOM_MB:-4096}"
# Reserve weights + OS headroom; rest → KV budget heuristic → ctx tokens
KV_BUDGET_MB=$((AVAIL_MB - MODEL_MB - HEADROOM_MB))
if [ "$KV_BUDGET_MB" -lt 2048 ]; then
  echo "ERROR: MemAvailable=${AVAIL_MB}MiB too tight for MODEL_MB=${MODEL_MB}" >&2
  exit 1
fi
# ~0.75 MiB/token heuristic for 35B-class MoE CPU KV (conservative)
CTX="${CTX:-$((KV_BUDGET_MB * 1024 * 1024 / 786432))}"
# clamp
[ "$CTX" -gt 131072 ] && CTX=131072
[ "$CTX" -lt 8192 ] && CTX=8192

pkill -f "llama-server.*--port ${PORT}" 2>/dev/null || true
sleep 2

{
  echo "port=${PORT} threads=${THREADS} avail_mb=${AVAIL_MB} model_mb=${MODEL_MB} ctx=${CTX}"
  echo "model=${MODEL}"
} | tee "$LOG"

exec "$LLAMA" \
  --model "$MODEL" \
  --port "$PORT" --host 127.0.0.1 \
  -dev none \
  --threads "$THREADS" \
  -np 1 \
  -c "$CTX" \
  --cache-ram -1 \
  --no-warmup \
  >>"$LOG" 2>&1
