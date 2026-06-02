#!/usr/bin/env bash
# Download Qwen3-Coder-Next (~80B) IQ3 GGUF for the second T440 P100 (:5002).
# Official weights: Qwen/Qwen3-Coder-Next — GGUF quants: bartowski/Qwen_Qwen3-Coder-Next-GGUF
#
# Default quant: IQ3_M (~37 GiB). Override: QUANT=IQ3_XS|IQ3_XXS
# Writable from cesarops2: /mnt/t440/data/cesarops/models (NFS to T440)
#
# Usage:
#   bash scripts/download_qwen3_coder_next_80b_iq3.sh
#   bash scripts/download_qwen3_coder_next_80b_iq3.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/download_qwen3_coder_next_80b_iq3.sh" ]] || REPO="/data/codebase/repos/wreckhunter2000-1"

QUANT="${QUANT:-IQ3_M}"
case "$QUANT" in
  IQ3_M)   FILE="Qwen_Qwen3-Coder-Next-IQ3_M.gguf" ;;
  IQ3_XS)  FILE="Qwen_Qwen3-Coder-Next-IQ3_XS.gguf" ;;
  IQ3_XXS) FILE="Qwen_Qwen3-Coder-Next-IQ3_XXS.gguf" ;;
  *) echo "Unknown QUANT=$QUANT (use IQ3_M, IQ3_XS, IQ3_XXS)" >&2; exit 1 ;;
esac

HF_REPO="bartowski/Qwen_Qwen3-Coder-Next-GGUF"
URL="https://huggingface.co/${HF_REPO}/resolve/main/${FILE}"

# Prefer T440 NFS data pool (writable from cesarops2); fall back to local spill.
DEST_DIR="${DEST_DIR:-/mnt/t440/data/cesarops/models}"
[[ -d "$DEST_DIR" ]] || DEST_DIR="${CESAROPS_DATA_ROOT:-$HOME/cesarops-data}/models"
mkdir -p "$DEST_DIR"

DEST="${DEST_DIR}/${FILE}"
LOG="${LOG:-/data/cesarops/logs/download-qwen3-coder-next-${QUANT}.log}"
PID_FILE="/tmp/download-qwen3-coder-next-${QUANT}.pid"

log() { echo "[coder-next-dl] $(date -Iseconds) $*" | tee -a "$LOG"; }

cmd_status() {
  if [[ -f "$DEST" ]]; then
    ls -lh "$DEST"
  else
    echo "missing: $DEST"
  fi
  if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    log "download running pid=$(cat "$PID_FILE")"
    tail -3 "$LOG" 2>/dev/null || true
  else
    log "no active download"
  fi
}

cmd_download() {
  if [[ -f "$DEST" ]]; then
    sz=$(stat -c%s "$DEST" 2>/dev/null || echo 0)
    if [[ "$sz" -gt 30000000000 ]]; then
      log "already present: $DEST ($(numfmt --to=iec "$sz" 2>/dev/null || echo "${sz}B"))"
      exit 0
    fi
  fi

  log "dest=$DEST"
  log "url=$URL"
  if [[ -f "$PID_FILE" ]] && kill -0 "$(cat "$PID_FILE")" 2>/dev/null; then
    log "already downloading pid=$(cat "$PID_FILE")"
    exit 0
  fi

  (
    set -e
    wget -c --progress=dot:giga -O "$DEST.part" "$URL"
    mv -f "$DEST.part" "$DEST"
    log "complete: $DEST"
  ) >>"$LOG" 2>&1 &
  echo $! >"$PID_FILE"
  log "started wget pid=$(cat "$PID_FILE") — tail -f $LOG"
}

case "${1:-download}" in
  status) cmd_status ;;
  download|"") cmd_download ;;
  *)
    echo "Usage: $0 {download|status}"
    exit 1
    ;;
esac
