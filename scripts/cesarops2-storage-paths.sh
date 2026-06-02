#!/usr/bin/env bash
# cesarops2 disk layout — source before downloads / HF sync.
#
#   source scripts/cesarops2-storage-paths.sh
#
# The small /data partition (sdc ~481G) fills fast. Primary local space is on / (sda ~1.8T).
set -euo pipefail

export CESAROPS_DATA_ROOT="${CESAROPS_DATA_ROOT:-$HOME/cesarops-data}"
export MODELS="${MODELS:-$CESAROPS_DATA_ROOT/models}"
export HF_HOME="${HF_HOME:-$HOME/.cache/huggingface}"
export HF_HUB_CACHE="${HF_HUB_CACHE:-$HF_HOME/hub}"

mkdir -p "$MODELS" "$HF_HUB_CACHE" "$CESAROPS_DATA_ROOT/logs"

# Local 110G disk (sdb) — top of mount only; NFS binds live in subdirs
export CESAROPS_SPILL="${CESAROPS_SPILL:-/mnt/data-external/local}"
mkdir -p "$CESAROPS_SPILL"

# Legacy 481G mount (often full) — avoid new writes here
export CESAROPS_DATA_LEGACY="${CESAROPS_DATA_LEGACY:-/data/cesarops}"

# Optional: T440 NFS pool (~1.1T free) — read/mirror only when not isolating
export CESAROPS_NFS_DATA="${CESAROPS_NFS_DATA:-/mnt/data-external/cesarops}"
