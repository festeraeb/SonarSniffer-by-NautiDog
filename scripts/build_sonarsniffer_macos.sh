#!/usr/bin/env bash
# macOS bundles via cargo-zigbuild (Docker image includes Apple SDK).
# Produces universal2 (Intel + Apple Silicon) when UNIVERSAL=1 (default).
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "$0")/.." && pwd)}"
source "$REPO/scripts/build_sonarsniffer_bundle_common.sh"

UNIVERSAL="${UNIVERSAL:-1}"
DOCKER_IMAGE="${DOCKER_IMAGE:-ghcr.io/rust-cross/cargo-zigbuild}"
OUT="${OUT:-$DIST/sonarsniffer-macos-universal}"
TARGET="${TARGET:-universal2-apple-darwin}"

if [[ "$UNIVERSAL" != "1" ]]; then
  OUT="${OUT:-$DIST/sonarsniffer-macos-arm64}"
  TARGET="${TARGET:-aarch64-apple-darwin}"
fi

source "${HOME}/.cargo/env" 2>/dev/null || true

for t in aarch64-apple-darwin x86_64-apple-darwin; do
  rustup target add "$t" 2>/dev/null || true
done

echo "[mac] pulling $DOCKER_IMAGE (if needed)"
docker pull "$DOCKER_IMAGE" >/dev/null 2>&1 || true

echo "[mac] zigbuild target=$TARGET (no GStreamer on cross-build)"
CARGO_DOCKER_HOME="${CARGO_DOCKER_HOME:-/tmp/cargo-zigbuild-home}"
mkdir -p "$CARGO_DOCKER_HOME"
docker run --rm \
  -v "$REPO:/io" -w /io/sonarsniffer \
  -v "$CARGO_DOCKER_HOME:/cargo-home" \
  -e CARGO_HOME=/cargo-home \
  -e CARGO_TARGET_DIR=/io/sonarsniffer/target-docker \
  "$DOCKER_IMAGE" \
  cargo zigbuild --release --target "$TARGET" --no-default-features \
    --bin sonarsniffer-cli --bin parse_cli

BIN_DIR="$SS_DIR/target-docker/$TARGET/release"
if [[ ! -f "$BIN_DIR/parse_cli" ]]; then
  BIN_DIR="$SS_DIR/target-docker/${TARGET%-apple-darwin}-apple-darwin/release"
fi
if [[ ! -f "$BIN_DIR/parse_cli" ]]; then
  # universal2 output path
  BIN_DIR="$SS_DIR/target-docker/universal2-apple-darwin/release"
fi
if [[ ! -f "$BIN_DIR/parse_cli" ]]; then
  echo "[mac] ERROR: binaries not found under target-docker; listing:"
  find "$SS_DIR/target-docker" -name parse_cli 2>/dev/null | head -5
  exit 1
fi

rm -rf "$OUT"
mkdir -p "$OUT/bin" "$OUT/test_files" "$OUT/docs"
cp "$BIN_DIR/sonarsniffer-cli" "$OUT/bin/"
cp "$BIN_DIR/parse_cli" "$OUT/bin/"
chmod +x "$OUT/bin/"*
bundle_test_files "$OUT/test_files"
bundle_docs "$OUT/docs"

cat >"$OUT/README.txt" <<'EOF'
SonarSniffer macOS bundle (cross-built from Linux)

Binaries:
  bin/sonarsniffer-cli  — probe sonar files
  bin/parse_cli         — pipeline without MP4 (no GStreamer in cross-build)

Quick test (Terminal):
  cd bin
  chmod +x sonarsniffer-cli parse_cli
  ./sonarsniffer-cli ../test_files
  ./parse_cli ../test_files/93SV-UHD-GT56.RSD --light --output-dir /tmp/ss-out

MP4 on macOS: install GStreamer, then rebuild natively:
  brew install gstreamer gst-plugins-base gst-plugins-good
  cargo build --release --features video-gstreamer
EOF

ZIP="$(make_zip "$OUT")"
echo "[mac] bundle: $OUT"
echo "[mac] zip:    $ZIP"
file "$OUT/bin/"* || true
