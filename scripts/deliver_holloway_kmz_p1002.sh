#!/usr/bin/env bash
# Deliver Holloway aligned KMZ to T440 temp (same /data subtree as laptopdump).
# Windows: X:\tmp\undercesarop\holloway_kmz_aligned\Holloway\track.kmz
# Linux:   /data/tmp/undercesarop/holloway_kmz_aligned/Holloway/track.kmz
set -euo pipefail
REPO="${REPO:-/mnt/t440/codebase/repos/wreckhunter2000-1}"
SRC="${SRC:-$REPO/outputs/holloway_kmz_aligned}"
HOST="${T440_HOST:-10.0.0.61}"
USER="${T440_USER:-cesarops}"
DEST="${DEST:-/data/tmp/undercesarop/holloway_kmz_aligned}"

ssh -o ConnectTimeout=10 "${USER}@${HOST}" "mkdir -p '${DEST}/Holloway'"
rsync -av "${SRC}/" "${USER}@${HOST}:${DEST}/"
echo "Delivered to ${USER}@${HOST}:${DEST}/Holloway/track.kmz"
echo "SMB (if X: = \\\\${HOST}\\data): X:\\tmp\\undercesarop\\holloway_kmz_aligned\\Holloway\\track.kmz"
