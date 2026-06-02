#!/usr/bin/env bash
# Build Zyphra vLLM (zaya1-pr) for cesarops2: Turing sm_75 (2060) + Pascal sm_61 (1070).
# P106 (6GB) is skipped — too small for ZAYA1-8B MoE shards in practice.
set -euo pipefail

VENV="${VENV:-${HOME}/.venvs/zaya-vllm}"
LOG="${LOG:-/tmp/install_zaya_vllm_c2.log}"
PYTHON="${PYTHON:-python3}"

export TORCH_CUDA_ARCH_LIST="${TORCH_CUDA_ARCH_LIST:-7.5;6.1}"
export MAX_JOBS="${MAX_JOBS:-4}"
export CUDA_HOME="${CUDA_HOME:-/usr/local/cuda-12.6}"
export PATH="${CUDA_HOME}/bin:${PATH}"
export LD_LIBRARY_PATH="${CUDA_HOME}/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

log() { echo "[$(date -u +%H:%M:%S)] [zaya-vllm-install] $*" | tee -a "$LOG"; }

: >"$LOG"
log "venv=$VENV TORCH_CUDA_ARCH_LIST=$TORCH_CUDA_ARCH_LIST"

if ! command -v nvcc >/dev/null 2>&1; then
  log "ERROR: nvcc not found (need CUDA toolkit)"
  exit 1
fi
log "nvcc: $(nvcc --version 2>&1 | grep release | head -1)"

if [[ ! -d "$VENV" ]]; then
  log "creating venv…"
  "$PYTHON" -m venv "$VENV"
fi
# shellcheck source=/dev/null
source "${VENV}/bin/activate"
pip install -U pip wheel setuptools
pip install -U "huggingface_hub[cli]"

log "installing Zyphra vLLM zaya1-pr (full source build, 30–90 min)…"
log "tail -f $LOG"
if ! pip install "vllm @ git+https://github.com/Zyphra/vllm.git@zaya1-pr" >>"$LOG" 2>&1; then
  log "ERROR: vLLM build failed — see $LOG"
  exit 1
fi

log "verify:"
"${VENV}/bin/vllm" --version 2>&1 | tee -a "$LOG" || true
log "done — activate: source ${VENV}/bin/activate"
