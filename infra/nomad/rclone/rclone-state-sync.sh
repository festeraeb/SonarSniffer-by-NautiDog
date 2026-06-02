#!/usr/bin/env bash
set -euo pipefail

SRC_ROOT="${SRC_ROOT:-/codebase/repos/wreckhunter2000-1}"
REMOTE="cesarops-backup:state"
export RCLONE_CONFIG="${RCLONE_CONFIG:-${HOME}/.config/rclone/rclone.conf}"

if ! command -v rclone >/dev/null 2>&1; then
  echo "SKIP: rclone not installed on this host"
  exit 0
fi

if [[ -f "$RCLONE_CONFIG" ]] && ! rclone --config "$RCLONE_CONFIG" listremotes &>/dev/null; then
  echo "ERROR: invalid rclone config ($RCLONE_CONFIG) — fix or run scripts/rclone-setup-t440.sh"
  exit 1
fi

if ! rclone listremotes 2>/dev/null | grep -q '^cesarops-backup:'; then
  echo "SKIP: rclone remote cesarops-backup not configured — run scripts/rclone-setup-t440.sh"
  exit 0
fi

rclone sync "$SRC_ROOT/docs" "$REMOTE/docs" --checksum --create-empty-src-dirs
rclone sync "$SRC_ROOT/infra/nomad" "$REMOTE/infra-nomad" --checksum --create-empty-src-dirs

# Optional artifact promotion path
if [[ -d /tmp/forge_compile_batch4_20260601T010941Z ]]; then
  rclone copy /tmp/forge_compile_batch4_20260601T010941Z "$REMOTE/artifacts/forge_compile_batch4_20260601T010941Z" --checksum
fi

echo "rclone state sync complete"
