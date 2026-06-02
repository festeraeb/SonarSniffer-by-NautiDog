#!/usr/bin/env bash
# Free space on cesarops2: retire /data writes, purge duplicates after R1 migrate.
#
#   bash scripts/cesarops2-disk-cleanup.sh           # safe defaults
#   bash scripts/cesarops2-disk-cleanup.sh --purge-legacy-r1  # after rsync to ~/cesarops-data
set -euo pipefail

REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
# shellcheck source=cesarops2-storage-paths.sh
source "$REPO/scripts/cesarops2-storage-paths.sh"

LEGACY_R1="/data/cesarops/models/DeepSeek-R1"
DEST_R1="$MODELS/DeepSeek-R1"
PURGE_R1="${PURGE_R1:-0}"
[[ "${1:-}" == *purge-legacy-r1* ]] && PURGE_R1=1

log() { echo "[disk-cleanup] $*"; }

log "=== before ==="
df -h / /data /mnt/data-external | grep -E 'Filesystem|/dev/'

# Duplicate HF cache on small /data (canonical: $HF_HOME on /)
if [[ -d /data/cesarops/hf_cache ]]; then
  log "removing /data/cesarops/hf_cache (use $HF_HOME)"
  rm -rf /data/cesarops/hf_cache
fi

# Move small GGUF off /data if present
mkdir -p "$MODELS"
for f in /data/cesarops/models/*.gguf; do
  [[ -f "$f" ]] || continue
  log "move $(basename "$f") → $MODELS/"
  mv -n "$f" "$MODELS/"
done

# Legacy DeepSeek copy on /data after successful migrate
if [[ "$PURGE_R1" == "1" && -d "$LEGACY_R1" ]]; then
  if [[ ! -d "$DEST_R1" ]]; then
    log "ERROR: $DEST_R1 missing — not purging $LEGACY_R1"
    exit 1
  fi
  src_n=$(find "$LEGACY_R1" -maxdepth 1 -name 'model-*.safetensors' | wc -l)
  dst_n=$(find "$DEST_R1" -maxdepth 1 -name 'model-*.safetensors' | wc -l)
  if [[ "$dst_n" -lt "$src_n" ]]; then
    log "ERROR: dest has $dst_n shards, legacy $src_n — wait for rsync or hf download"
    exit 1
  fi
  log "purging legacy R1 on /data ($src_n shards, ~$(du -sh "$LEGACY_R1" | cut -f1))"
  rm -rf "$LEGACY_R1"
fi

# Hint file on /data so scripts stop using it
mkdir -p /data/cesarops
if [[ ! -f /data/cesarops/README_DO_NOT_WRITE ]]; then
  cat >/data/cesarops/README_DO_NOT_WRITE <<EOF
This partition (/dev/sdc ~481G) is legacy and often full.
Use instead:
  $CESAROPS_DATA_ROOT  (models, logs)
  $HF_HOME             (HuggingFace hub)
  /mnt/data-external   (local 110G spill, top level only — subdirs may be NFS)
EOF
fi

log "=== after ==="
df -h / /data /mnt/data-external | grep -E 'Filesystem|/dev/'
log "primary models: $MODELS"
log "to free ~350G more after R1 rsync: bash $0 --purge-legacy-r1"
