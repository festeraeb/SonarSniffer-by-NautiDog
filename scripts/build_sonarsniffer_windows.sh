#!/usr/bin/env bash
# Cross-build SonarSniffer CLIs for Windows + bundle a test zip.
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "$0")/.." && pwd)}"
source "$REPO/scripts/build_sonarsniffer_bundle_common.sh"

OUT="${OUT:-$DIST/sonarsniffer-windows-x64}"
TARGET="${TARGET:-x86_64-pc-windows-gnu}"

source "${HOME}/.cargo/env" 2>/dev/null || true

if ! rustup target list --installed | grep -q "$TARGET"; then
  echo "[win] adding target $TARGET"
  rustup target add "$TARGET"
fi

if ! command -v x86_64-w64-mingw32-gcc >/dev/null 2>&1; then
  echo "[win] install: sudo apt install gcc-mingw-w64-x86-64"
  exit 1
fi

export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc

cd "$SS_DIR"
echo "[win] building (no GStreamer on cross-build — video needs Windows GStreamer runtime)"
cargo build --release --target "$TARGET" --no-default-features \
  --bin sonarsniffer-cli --bin parse_cli

rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/test_files" "$OUT/docs"

cp "target/$TARGET/release/sonarsniffer-cli.exe" "$OUT/bin/"
cp "target/$TARGET/release/parse_cli.exe" "$OUT/bin/"
bundle_test_files "$OUT/test_files"
bundle_docs "$OUT/docs"

cat >"$OUT/README.txt" <<'EOF'
SonarSniffer Windows x64 bundle (cross-built from Linux)

Binaries:
  bin\sonarsniffer-cli.exe  — probe sonar files
  bin\parse_cli.exe         — full pipeline (no MP4 without GStreamer on Windows)

Quick test (PowerShell):
  cd bin
  .\sonarsniffer-cli.exe ..\test_files
  .\parse_cli.exe ..\test_files\93SV-UHD-GT56.RSD --light --output-dir ..\out

MP4 export on Windows:
  Install GStreamer MSVC runtime (full):
  https://gstreamer.freedesktop.org/download/
  Then rebuild on Windows with: cargo build --release --features video-gstreamer
EOF

ZIP="$(make_zip "$OUT")"
echo "[win] bundle: $OUT"
echo "[win] zip:    $ZIP"
ls -la "$OUT/bin"
