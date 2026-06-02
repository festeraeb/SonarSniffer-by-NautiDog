#!/usr/bin/env bash
# Download / link GGUF (MTP + Gemma) and vision HF models for T440 + cesarops2 NFS.
#
# Usage:
#   bash scripts/download_cluster_models.sh gguf      # symlinks + optional HF GGUF pulls
#   bash scripts/download_cluster_models.sh vision    # Florence-2-base + Moondream2
#   bash scripts/download_cluster_models.sh all
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
MODELS_SHARED="${MODELS_SHARED:-/codebase/models}"
LOCAL="${LOCAL_MODELS:-/data/cesarops/local_models}"
VISION="${VISION_MODELS:-/data/cesarops/vision_models}"
HF="${HF:-hf}"

log() { echo "[download] $*"; }

link_gguf() {
  local src=$1 name=$2
  if [[ ! -f "$src" ]]; then
    log "MISSING $name — $src"
    return 1
  fi
  ln -sfn "$src" "$MODELS_SHARED/$name"
  log "OK $MODELS_SHARED/$name -> $src"
}

download_gguf_hf() {
  local repo=$1 file=$2 dest=$3
  mkdir -p "$dest"
  if [[ -f "$dest/$file" ]]; then
    log "exists $dest/$file"
    return 0
  fi
  log "pull $repo / $file"
  "$HF" download "$repo" "$file" --local-dir "$dest"
}

cmd_gguf() {
  mkdir -p "$MODELS_SHARED" "$LOCAL"
  # Already on disk — expose via NFS share
  link_gguf "$LOCAL/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf" \
    "Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf" || true
  link_gguf "$LOCAL/Qwopus3.5-9B-Coder-MTP-Q4_K_M.gguf" \
    "Qwopus3.5-9B-Coder-MTP-Q4_K_M.gguf" || true
  link_gguf "$LOCAL/Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf" \
    "Qwen3.6-35B-A3B-UD-Q4_K_XL.gguf" || true
  link_gguf "$LOCAL/Qwen3.6-27B-MTP-UD-Q4_K_XL.gguf" \
    "Qwen3.6-27B-MTP-UD-Q4_K_XL.gguf" || true
  if [[ -f "$MODELS_SHARED/Gemma-4-26B-MoE-IQ4_XS.gguf" ]]; then
    log "OK Gemma-4-26B-MoE-IQ4_XS.gguf in $MODELS_SHARED"
  else
    log "WARN Gemma IQ4_XS not in $MODELS_SHARED — download separately"
  fi
  # Optional fresh pull if missing
  if [[ ! -f "$LOCAL/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf" ]]; then
    download_gguf_hf \
      "Jackrong/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-GGUF" \
      "Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf" \
      "$LOCAL"
    link_gguf "$LOCAL/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf" \
      "Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_M.gguf"
  fi
  if [[ ! -f "$LOCAL/Qwopus3.5-9B-Coder-MTP-Q4_K_M.gguf" ]]; then
    download_gguf_hf \
      "Jackrong/Qwen3.5-9B-DeepSeek-V4-Flash-MTP-GGUF" \
      "Qwen3.5-9B-DeepSeek-V4-Flash-MTP-Q4_K_S.gguf" \
      "$LOCAL" || true
  fi
  log "GGUF layout:"
  ls -lh "$MODELS_SHARED"/*MTP* "$MODELS_SHARED"/Gemma-4-26B-MoE-IQ4_XS.gguf 2>/dev/null || true
}

cmd_vision() {
  mkdir -p "$VISION"
  export HF_HOME="${HF_HOME:-/data/cesarops/hf_cache}"
  mkdir -p "$HF_HOME"
  log "HF_HOME=$HF_HOME"
  # Florence-2-base (~0.5GB) — fits 8GB GPU workers; large optional later
  if [[ ! -d "$VISION/Florence-2-base" ]]; then
    log "Downloading microsoft/Florence-2-base ..."
    "$HF" download microsoft/Florence-2-base --local-dir "$VISION/Florence-2-base"
  else
    log "OK Florence-2-base"
  fi
  if [[ ! -d "$VISION/moondream2" ]]; then
    log "Downloading vikhyatk/moondream2 ..."
    "$HF" download vikhyatk/moondream2 --local-dir "$VISION/moondream2"
  else
    log "OK moondream2"
  fi
  log "Vision models in $VISION"
  du -sh "$VISION"/* 2>/dev/null || true
}

case "${1:-all}" in
  gguf)   cmd_gguf ;;
  vision) cmd_vision ;;
  all)    cmd_gguf; cmd_vision ;;
  *)
    echo "Usage: $0 {gguf|vision|all}"
    exit 1
    ;;
esac

log "Done."
