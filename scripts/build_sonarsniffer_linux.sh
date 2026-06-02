#!/usr/bin/env bash
# Native Linux x86_64 bundle (GStreamer / MP4 when runtime installed).
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "$0")/.." && pwd)}"
source "$REPO/scripts/build_sonarsniffer_bundle_common.sh"

OUT="${OUT:-$DIST/sonarsniffer-linux-x64}"
TARGET="${TARGET:-x86_64-unknown-linux-gnu}"

source "${HOME}/.cargo/env" 2>/dev/null || true

cd "$SS_DIR"
echo "[linux] release build (video-gstreamer)"
cargo build --release --bin sonarsniffer-cli --bin parse_cli

rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/test_files" "$OUT/docs"

cp "target/release/sonarsniffer-cli" "$OUT/bin/"
cp "target/release/parse_cli" "$OUT/bin/"
bundle_test_files "$OUT/test_files"
bundle_docs "$OUT/docs"

cat >"$OUT/README.txt" <<'EOF'
SonarSniffer Linux x86_64 bundle

Binaries:
  bin/sonarsniffer-cli  — probe sonar files
  bin/parse_cli         — full pipeline (mosaic, waterfall, KML, MP4 with GStreamer)

Quick test:
  cd bin
  ./sonarsniffer-cli ../test_files
  ./parse_cli ../test_files/93SV-UHD-GT56.RSD --light --output-dir /tmp/ss-out

MP4 export requires GStreamer 1.x dev/runtime on the host.
EOF

ZIP="$(make_zip "$OUT")"
echo "[linux] bundle: $OUT"
echo "[linux] zip:    $ZIP"
ls -la "$OUT/bin"
