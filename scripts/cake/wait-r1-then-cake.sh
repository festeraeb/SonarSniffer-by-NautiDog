#!/usr/bin/env bash
# Wait for DeepSeek-R1 download, then start Cake.
# Disabled by default — set START_CAKE_WHEN_DONE=1 to auto-launch.
set -euo pipefail
START_CAKE_WHEN_DONE="${START_CAKE_WHEN_DONE:-0}"
REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
DEST="${DEST:-$HOME/cesarops-data/models/DeepSeek-R1}"
LOG="${LOG:-$HOME/cesarops-data/logs/r1-then-cake.log}"
TOTAL="${TOTAL:-163}"

log() { echo "[r1→cake] $(date -u +%H:%M:%S) $*" | tee -a "$LOG"; }

log "watching $DEST for $TOTAL shards"
while true; do
  n=$(find "$DEST" -maxdepth 1 -name 'model-*.safetensors' 2>/dev/null | wc -l)
  du=$(du -sh "$DEST" 2>/dev/null | cut -f1 || echo "?")
  if pgrep -af 'hf download.*DeepSeek-R1' >/dev/null; then
    st="hf downloading"
  elif pgrep -af 'rsync.*DeepSeek-R1' >/dev/null; then
    st="rsync"
  else
    st="idle"
  fi
  log "shards $n/$TOTAL size=$du ($st)"
  [[ "$n" -ge "$TOTAL" ]] && [[ -f "$DEST/config.json" ]] && break
  sleep 120
done

log "download complete"
bash "$REPO/scripts/cesarops2-disk-cleanup.sh" --purge-legacy-r1 2>&1 | tee -a "$LOG" || true

if [[ "$START_CAKE_WHEN_DONE" == "1" ]]; then
  log "START_CAKE_WHEN_DONE=1 — starting Cake R1"
  bash "$REPO/scripts/cake/start-cake-r1-c2.sh" start 2>&1 | tee -a "$LOG"
else
  log "Cake held off (set START_CAKE_WHEN_DONE=1 to auto-start)"
fi
