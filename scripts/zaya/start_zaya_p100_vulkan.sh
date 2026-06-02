#!/usr/bin/env bash
# Start ZAYA1-8B on a P100 host via llama.cpp Vulkan.
set -euo pipefail

PORT="${PORT:-5001}"
HOST="${HOST:-0.0.0.0}"
CTX="${CTX:-2048}"
NGL="${NGL:-999}"
THREADS="${THREADS:-8}"
DEVICE="${DEVICE:-Vulkan0}"
LOG="${LOG:-/tmp/zaya-p100-vulkan-${PORT}.log}"
MODEL_DIR="${MODEL_DIR:-}"

pick_first_existing() {
  local p
  for p in "$@"; do
    if [[ -f "$p" ]]; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

pick_first_executable() {
  local p
  for p in "$@"; do
    if [[ -x "$p" ]]; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

pick_first_dir() {
  local p
  for p in "$@"; do
    if [[ -d "$p" ]]; then
      echo "$p"
      return 0
    fi
  done
  return 1
}

pick_hf_bin() {
  pick_first_executable \
    "${HF_BIN:-}" \
    /home/cesarops/.venvs/hf/bin/hf \
    /home/cesarops/.venvs/zaya-vllm/bin/hf \
    /data/cesarops/venvs/zaya-p100/bin/hf \
    /usr/local/bin/hf \
    /usr/bin/hf \
    hf || true
}

LLAMA_BIN="$(pick_first_executable \
  /home/cesarops/src/llama.cpp-zaya/build-vk/bin/llama-server \
  /home/cesarops/src/llama.cpp-zaya/build/bin/llama-server \
  /home/cesarops/src/llama.cpp/build/bin/llama-server \
  /codebase/src/llama.cpp-zaya/build-vk/bin/llama-server \
  /codebase/src/llama.cpp-zaya/build/bin/llama-server \
  /codebase/src/llama.cpp/build/bin/llama-server \
  )"

if [[ -z "${LLAMA_BIN:-}" ]]; then
  echo "[zaya-p100-vulkan] llama-server binary not found" >&2
  exit 1
fi

if [[ -z "$MODEL_DIR" ]]; then
  MODEL_DIR="$(pick_first_dir \
    /mnt/data-external/cesarops/models \
    /mnt/t440/home/cesarops/cesarops-data/models \
    /data/cesarops/models \
    /home/cesarops/cesarops-data/models \
    /home/cesarops/models \
    /codebase/models \
    /mnt/t440/codebase/models \
    /mnt/t440/models \
    )"
fi

if [[ -z "${MODEL_DIR:-}" ]]; then
  echo "[zaya-p100-vulkan] no model directory found" >&2
  exit 1
fi

MODEL="$(pick_first_existing \
  "${MODEL_DIR}/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q8_0.gguf" \
  "${MODEL_DIR}/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q6_K.gguf" \
  "${MODEL_DIR}/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q5_K_M.gguf" \
  "${MODEL_DIR}/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q4_K_M.gguf" \
  "${MODEL_DIR}/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q8_0.gguf" \
  "${MODEL_DIR}/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q6_K.gguf" \
  "${MODEL_DIR}/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q5_K_M.gguf" \
  "${MODEL_DIR}/Zyphra/Abiray-ZAYA1-8B-GGUF/ZAYA1-8B-Q4_K_M.gguf" \
  "${MODEL_DIR}/ZAYA1-8B-Q8_0.gguf" \
  "${MODEL_DIR}/ZAYA1-8B-Q6_K.gguf" \
  "${MODEL_DIR}/ZAYA1-8B-Q5_K_M.gguf" \
  "${MODEL_DIR}/ZAYA1-8B-Q4_K_M.gguf" \
  /codebase/models/ZAYA1-8B-Q8_0.gguf \
  /mnt/t440/codebase/models/ZAYA1-8B-Q8_0.gguf \
  /codebase/models/ZAYA1-8B-Q6_K.gguf \
  /mnt/t440/codebase/models/ZAYA1-8B-Q6_K.gguf \
  /codebase/models/ZAYA1-8B-Q5_K_M.gguf \
  /mnt/t440/codebase/models/ZAYA1-8B-Q5_K_M.gguf \
  /home/cesarops/cesarops-data/models/ZAYA1-8B-Q4_K_M.gguf \
  /codebase/models/ZAYA1-8B-Q4_K_M.gguf \
  /mnt/t440/codebase/models/ZAYA1-8B-Q4_K_M.gguf \
  )"

if [[ -z "${MODEL:-}" ]]; then
  HF_FOUND="$(pick_hf_bin)"
  if [[ -n "${HF_FOUND:-}" ]]; then
    mkdir -p "$MODEL_DIR"
    echo "[zaya-p100-vulkan] downloading ZAYA1-8B-Q8_0.gguf into $MODEL_DIR"
    "$HF_FOUND" download Abiray/ZAYA1-8B-GGUF --include 'ZAYA1-8B-Q8_0.gguf' --local-dir "$MODEL_DIR" --max-workers 4
  fi
  MODEL="$(pick_first_existing \
    "${MODEL_DIR}/ZAYA1-8B-Q8_0.gguf" \
    "${MODEL_DIR}/ZAYA1-8B-Q6_K.gguf" \
    "${MODEL_DIR}/ZAYA1-8B-Q5_K_M.gguf" \
    "${MODEL_DIR}/ZAYA1-8B-Q4_K_M.gguf" \
    )"
  if [[ -z "${MODEL:-}" ]]; then
    echo "[zaya-p100-vulkan] no ZAYA1-8B GGUF model found (or download failed)" >&2
    exit 1
  fi
fi

echo "[zaya-p100-vulkan] bin=$LLAMA_BIN model=$MODEL dev=$DEVICE port=$PORT log=$LOG"

fuser -k "${PORT}/tcp" 2>/dev/null || true
pkill -f "llama-server.*${PORT}" 2>/dev/null || true
sleep 1

nohup "$LLAMA_BIN" \
  -m "$MODEL" \
  --host "$HOST" \
  --port "$PORT" \
  -dev "$DEVICE" \
  -ngl "$NGL" \
  -c "$CTX" \
  -np 1 \
  -t "$THREADS" \
  >"$LOG" 2>&1 &

for _ in $(seq 1 40); do
  if curl -sS --max-time 2 "http://127.0.0.1:${PORT}/health" >/dev/null 2>&1; then
    echo "[zaya-p100-vulkan] ready http://127.0.0.1:${PORT}/health"
    exit 0
  fi
  sleep 2
done

echo "[zaya-p100-vulkan] failed to become ready; tail $LOG" >&2
tail -n 60 "$LOG" >&2 || true
exit 1
