#!/usr/bin/env bash
# Shared helpers for SonarSniffer platform bundles.
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
SS_DIR="$REPO/sonarsniffer"
DIST="${DIST:-$REPO/dist}"
# Small smoke-test sample only (full test_files/ is multi-GB).
SAMPLE_RSD="${SAMPLE_RSD:-93SV-UHD-GT56.RSD}"

bundle_test_files() {
  local dest="$1"
  mkdir -p "$dest"
  if [[ -f "$SS_DIR/test_files/$SAMPLE_RSD" ]]; then
    cp "$SS_DIR/test_files/$SAMPLE_RSD" "$dest/"
  fi
}

bundle_docs() {
  local dest="$1"
  mkdir -p "$dest"
  cp "$REPO/scripts/analyze_sonarsniffer_outputs.py" "$dest/" 2>/dev/null || true
}

make_zip() {
  local dir="$1"
  local zip="${dir}.zip"
  rm -f "$zip"
  if command -v zip >/dev/null 2>&1; then
    (cd "$(dirname "$dir")" && zip -qr "$(basename "$zip")" "$(basename "$dir")")
  else
    python3 -c "import shutil; shutil.make_archive('${dir}', 'zip', root_dir='${dir}')"
  fi
  echo "$zip"
}
