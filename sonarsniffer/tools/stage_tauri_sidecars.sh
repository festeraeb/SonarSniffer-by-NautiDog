#!/usr/bin/env bash
# Stage CLI sidecars for Tauri bundling (Linux/macOS).
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PROFILE="${1:-release}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-${GITHUB_ACTIONS:+${REPO_ROOT}/target}}"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$HOME/.sonarsniffer-build/target}"

cd "$REPO_ROOT"
TRIPLE="$(rustc --print host-tuple)"
echo "Building CLI sidecars ($PROFILE) -> $CARGO_TARGET_DIR ($TRIPLE)"
cargo build --"$PROFILE" --no-default-features --bin sonarsniffer-cli --bin parse_cli
cargo build --"$PROFILE" -p soundtiles --bin soundtiles

BIN_DIR="$REPO_ROOT/desktop/src-tauri/binaries"
mkdir -p "$BIN_DIR"

for name in sonarsniffer-cli parse_cli soundtiles; do
  src="$CARGO_TARGET_DIR/$PROFILE/$name"
  dst="$BIN_DIR/$name-$TRIPLE"
  if [[ ! -f "$src" ]]; then
    echo "Missing build output: $src" >&2
    exit 1
  fi
  cp "$src" "$dst"
  chmod +x "$dst"
  echo "  staged $dst"
done

echo "Sidecars ready in desktop/src-tauri/binaries/"
