#!/usr/bin/env bash
# Build Linux, Windows, and macOS SonarSniffer release bundles.
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "$0")/.." && pwd)}"
export REPO DIST="${DIST:-$REPO/dist}"

mkdir -p "$DIST"
chmod +x "$REPO"/scripts/build_sonarsniffer_*.sh

echo "=== SonarSniffer multi-platform build ==="
echo "dist: $DIST"
date -u +"%Y-%m-%dT%H:%M:%SZ"

bash "$REPO/scripts/build_sonarsniffer_linux.sh"
bash "$REPO/scripts/build_sonarsniffer_windows.sh"
bash "$REPO/scripts/build_sonarsniffer_macos.sh"

echo ""
echo "=== Artifacts ==="
ls -lah "$DIST"/sonarsniffer-*.zip 2>/dev/null || ls -lah "$DIST"/
