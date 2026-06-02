#!/usr/bin/env bash
# Download barozp NF4 ZAYA1-8B (~5GB) — CUDA-friendly quant; Osaurus MXFP4 is Apple MLX only.
set -euo pipefail

DEST="${DEST:-/data/cesarops/models/barozp-ZAYA1-8B-NF4/NF4}"
LOG="${LOG:-/data/cesarops/logs/zaya-nf4-download.log}"
HF_BIN="${HF_BIN:-/data/cesarops/venvs/zaya-p100/bin/hf}"

log() { echo "[zaya-nf4] $*" | tee -a "$LOG"; }

mkdir -p "$(dirname "$LOG")" "$(dirname "$DEST")"

if [[ -f "${DEST}/config.json" ]]; then
  log "already present: ${DEST}"
  exit 0
fi

if [[ ! -x "$HF_BIN" ]]; then
  HF_BIN="hf"
fi

log "downloading barozp/ZAYA1-8B-BNB NF4/ -> ${DEST}"
"$HF_BIN" download barozp/ZAYA1-8B-BNB --include "NF4/*" --local-dir "$(dirname "$(dirname "$DEST")")/barozp-ZAYA1-8B-NF4" --max-workers 4 2>&1 | tee -a "$LOG"
log "done: ${DEST}"
