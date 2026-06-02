#!/usr/bin/env bash
# DeepSeek-R1 full weights (~685GB) on the 1.8T system disk — NOT /data (481G, often full).
#
# Usage:
#   bash scripts/download-deepseek-r1.sh
#   bash scripts/download-deepseek-r1.sh status
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
[[ -f "$REPO/scripts/cesarops2-storage-paths.sh" ]] && source "$REPO/scripts/cesarops2-storage-paths.sh"

DEST="${DEST:-$HOME/cesarops-data/models/DeepSeek-R1}"
LEGACY="${LEGACY:-/data/cesarops/models/DeepSeek-R1}"
HF_BIN="${HF_BIN:-/data/cesarops/venvs/hf/bin/hf}"
[[ -x "$HF_BIN" ]] || HF_BIN="$(command -v hf || true)"
LOG="${LOG:-$HOME/cesarops-data/logs/deepseek-r1-download.log}"
REPO_ID="deepseek-ai/DeepSeek-R1"
TOTAL_SHARDS=163

log() {
  local line="[deepseek-r1] $(date -u +%H:%M:%S) $*"
  echo "$line"
  echo "$line" >>"$LOG"
}

shard_count() {
  find "${1:-$DEST}" -maxdepth 1 -name 'model-*.safetensors' 2>/dev/null | wc -l
}

cmd_status() {
  mkdir -p "$(dirname "$LOG")" "$(dirname "$DEST")"
  local n dest_gb legacy_gb
  n=$(shard_count "$DEST")
  dest_gb="0"
  legacy_gb="none"
  [[ -d "$DEST" ]] && dest_gb=$(du -sh "$DEST" 2>/dev/null | cut -f1 || echo "?")
  [[ -d "$LEGACY" ]] && legacy_gb=$(du -sh "$LEGACY" 2>/dev/null | cut -f1 || echo "?")
  log "DEST=$DEST shards=$n/$TOTAL_SHARDS size=$dest_gb"
  log "LEGACY=$LEGACY size=$legacy_gb (migrate source if shards here only)"
  df -h "$HOME" | tail -1 | awk '{print "[deepseek-r1] disk "$1" avail="$4" use="$5}'
  pgrep -af 'hf download.*DeepSeek-R1' && echo "[deepseek-r1] download running" || echo "[deepseek-r1] download not running"
  pgrep -af 'rsync.*DeepSeek-R1' && echo "[deepseek-r1] rsync running" || true
}

migrate_legacy() {
  [[ -d "$LEGACY" ]] || return 0
  local src_n dst_n
  src_n=$(shard_count "$LEGACY")
  dst_n=$(shard_count "$DEST")
  [[ "$src_n" -gt 0 ]] || return 0
  if [[ "$dst_n" -ge "$src_n" ]]; then
    log "skip migrate — DEST already has $dst_n shards (legacy $src_n)"
    return 0
  fi
  mkdir -p "$DEST"
  log "migrating ~${src_n} shards from $LEGACY → $DEST (1.4T disk)…"
  rsync -a --info=progress2 "$LEGACY/" "$DEST/" 2>&1 | tee -a "$LOG"
  log "migrate done — shards now $(shard_count "$DEST")"
}

cmd_download() {
  mkdir -p "$(dirname "$LOG")" "$(dirname "$DEST")"
  if ! [[ -x "$HF_BIN" ]]; then
    log "ERROR: hf CLI missing ($HF_BIN)"
    exit 1
  fi
  export HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
  migrate_legacy
  local n
  n=$(shard_count)
  log "resuming Hub download → $DEST (shards $n/$TOTAL_SHARDS, HF_HOME=$HF_HOME)"
  df -h "$HOME" | tail -1 | tee -a "$LOG"
  "$HF_BIN" download "$REPO_ID" --local-dir "$DEST" 2>&1 | tee -a "$LOG"
  n=$(shard_count)
  log "finished — shards $n/$TOTAL_SHARDS at $DEST"
}

case "${1:-download}" in
  status) cmd_status ;;
  migrate) migrate_legacy ;;
  download|*) cmd_download ;;
esac
