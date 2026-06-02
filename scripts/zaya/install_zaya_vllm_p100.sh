#!/usr/bin/env bash
# Build Zyphra vLLM (zaya1-pr) for T440 dual P100 (Pascal sm_60).
set -euo pipefail

VENV="${VENV:-/data/cesarops/venvs/zaya-p100}"
LOG="${LOG:-/data/cesarops/logs/install_zaya_vllm_p100.log}"
PYTHON="${PYTHON:-/data/cesarops/venvs/zaya-p100/bin/python}"

export TORCH_CUDA_ARCH_LIST="${TORCH_CUDA_ARCH_LIST:-6.0}"
export MAX_JOBS="${MAX_JOBS:-4}"
export CUDA_HOME="${CUDA_HOME:-/usr/local/cuda-12.8}"
export PATH="${CUDA_HOME}/bin:${PATH}"
export LD_LIBRARY_PATH="${CUDA_HOME}/lib64${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

log() { echo "[$(date -u +%H:%M:%S)] [zaya-vllm-p100] $*" | tee -a "$LOG"; }

mkdir -p "$(dirname "$LOG")"
: >"$LOG"
log "venv=$VENV TORCH_CUDA_ARCH_LIST=$TORCH_CUDA_ARCH_LIST"

if ! command -v nvcc >/dev/null 2>&1; then
  log "ERROR: nvcc not found (need CUDA toolkit)"
  exit 1
fi
log "nvcc: $(nvcc --version 2>&1 | grep release | head -1)"

if [[ ! -d "$VENV" ]]; then
  log "creating venv…"
  python3 -m venv "$VENV"
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
