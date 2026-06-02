#!/usr/bin/env bash
set -euo pipefail

REPO_PATH="${1:-/codebase/wreckhunter2000-1}"

if [[ "${CESAROPS_ALLOW_LOCAL_REPO:-0}" == "1" ]]; then
  if [[ -d "$REPO_PATH" && -f "$REPO_PATH/infra/nomad/scripts/bootstrap_client.sh" ]]; then
    echo "WARN: CESAROPS_ALLOW_LOCAL_REPO=1 — skipping mount preflight for $REPO_PATH" >&2
    echo "repo_path=$REPO_PATH"
    echo "mount_target=local"
    echo "OK: local-repo preflight (nautik9 emergency only)"
    exit 0
  fi
fi

if [[ ! -d "$REPO_PATH" ]]; then
  if [[ -d "/data/codebase/repos/wreckhunter2000-1" ]]; then
    echo "WARN: repo not found at $REPO_PATH; falling back to /data/codebase/repos/wreckhunter2000-1" >&2
    REPO_PATH="/data/codebase/repos/wreckhunter2000-1"
  else
    echo "ERROR: repo path not found: $REPO_PATH" >&2
    exit 2
  fi
fi

MOUNT_INFO=$(findmnt -T "$REPO_PATH" -n -o TARGET,SOURCE,FSTYPE || true)
if [[ -z "$MOUNT_INFO" ]]; then
  echo "ERROR: unable to resolve mount info for $REPO_PATH" >&2
  exit 3
fi

TARGET=$(awk '{print $1}' <<<"$MOUNT_INFO")
SOURCE=$(awk '{print $2}' <<<"$MOUNT_INFO")
FSTYPE=$(awk '{print $3}' <<<"$MOUNT_INFO")

echo "repo_path=$REPO_PATH"
echo "mount_target=$TARGET"
echo "mount_source=$SOURCE"
echo "mount_fstype=$FSTYPE"

if [[ "$TARGET" == "/" ]]; then
  echo "ERROR: repo resolves to root filesystem, not a dedicated mount" >&2
  exit 4
fi

case "$FSTYPE" in
  nfs|nfs4|ext4|xfs|zfs|btrfs)
    ;;
  *)
    echo "ERROR: unexpected filesystem type for rollout: $FSTYPE" >&2
    exit 5
    ;;
esac

# Bind mount over NFS (WSL pattern: mount /codebase then --bind repo subpath)
if [[ "$FSTYPE" != nfs* ]]; then
  parent_mount=$(findmnt -T "$REPO_PATH" -n -o FSTYPE 2>/dev/null | head -1)
  if [[ "$parent_mount" == nfs4 || "$parent_mount" == nfs ]]; then
    echo "OK: repo on bind mount over $parent_mount"
    exit 0
  fi
fi

echo "OK: mounted-drive preflight passed"
