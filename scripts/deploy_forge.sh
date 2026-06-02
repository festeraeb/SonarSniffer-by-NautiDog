#!/usr/bin/env bash
# Deploy cesarops-forge-v2 from CARGO_TARGET_DIR (T440 uses /data/cargo-target).
set -euo pipefail
ROOT="${ROOT:-/codebase/repos/wreckhunter2000-1}"
TARGET_DIR="${CARGO_TARGET_DIR:-/data/cargo-target}"
SRC="$TARGET_DIR/release/cesarops-forge-v2"
DEST="${DEST:-/tmp/forge-install/bin/cesarops-forge-v2}"

cd "$ROOT"
cargo build --release -p cesarops-forge-v2
[[ -x "$SRC" ]] || { echo "missing $SRC"; exit 1; }
mkdir -p "$(dirname "$DEST")"
cp -f "$SRC" "$DEST"
SYS_DEST="$ROOT/cesarops-forge-v2/target/release/cesarops-forge-v2"
mkdir -p "$(dirname "$SYS_DEST")"
cp -f "$SRC" "$SYS_DEST"
echo "deployed $SRC -> $DEST and $SYS_DEST ($(ls -lh "$DEST" | awk '{print $5,$6,$7,$8}'))"
