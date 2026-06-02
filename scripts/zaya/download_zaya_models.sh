#!/usr/bin/env bash
# Download ZAYA1-8B (text/MoE) and ZAYA1-VL-8B (vision) weights for cesarops2.
set -euo pipefail

MODEL_ROOT="${MODEL_ROOT:-${HOME}/models/Zyphra}"
HF_BIN="${HF_BIN:-}"
if [[ -z "$HF_BIN" ]]; then
  if [[ -x "${HOME}/.venvs/hf/bin/hf" ]]; then
    HF_BIN="${HOME}/.venvs/hf/bin/hf"
  elif [[ -x "${HOME}/.venvs/zaya-vllm/bin/hf" ]]; then
    HF_BIN="${HOME}/.venvs/zaya-vllm/bin/hf"
  else
    HF_BIN="hf"
  fi
fi

log() { echo "[zaya-download] $*"; }

download_one() {
  local repo=$1
  local dest="${MODEL_ROOT}/$(basename "$repo")"
  if [[ -f "${dest}/config.json" ]]; then
    log "already present: ${dest}"
    return 0
  fi
  mkdir -p "$dest"
  log "downloading ${repo} -> ${dest}"
  "$HF_BIN" download "$repo" --local-dir "$dest" --max-workers 4
  log "done: ${dest}"
}

main() {
  command -v "$HF_BIN" >/dev/null 2>&1 || {
    log "hf CLI missing — pip install huggingface_hub[cli] in ~/.venvs/zaya-vllm"
    exit 1
  }
  mkdir -p "$MODEL_ROOT"
  download_one "Zyphra/ZAYA1-8B"
  download_one "Zyphra/ZAYA1-VL-8B"
  log "models under ${MODEL_ROOT}"
}

main "$@"
