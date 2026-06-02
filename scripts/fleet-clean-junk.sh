#!/usr/bin/env bash
# Safe cleanup for generated artifacts while preserving runtime directories.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
ARCHIVE_ROOT="${ARCHIVE_ROOT:-${REPO}/outputs/forge_dispatch_archive}"
KEEP_DAYS="${KEEP_DAYS:-30}"

cd "$REPO"

mkdir -p "${REPO}/cesarops-forge-v2/dispatch_results"
: > "${REPO}/cesarops-forge-v2/dispatch_results/.gitkeep"

if [[ -d "${REPO}/cesarops-forge-v2/.cursor" ]]; then
  rm -rf "${REPO}/cesarops-forge-v2/.cursor"
fi

if [[ -d "${REPO}/cesarops-forge-v2/target" ]]; then
  rm -rf "${REPO}/cesarops-forge-v2/target"
fi

mkdir -p "$ARCHIVE_ROOT"
if find "${REPO}/cesarops-forge-v2/dispatch_results" -maxdepth 1 -type f ! -name '.gitkeep' | grep -q .; then
  day="$(date +%Y-%m-%d)"
  mkdir -p "${ARCHIVE_ROOT}/${day}"
  find "${REPO}/cesarops-forge-v2/dispatch_results" -maxdepth 1 -type f ! -name '.gitkeep' -exec mv -t "${ARCHIVE_ROOT}/${day}" {} +
fi

find "$ARCHIVE_ROOT" -mindepth 1 -maxdepth 1 -type d -mtime "+${KEEP_DAYS}" -exec rm -rf {} + 2>/dev/null || true
