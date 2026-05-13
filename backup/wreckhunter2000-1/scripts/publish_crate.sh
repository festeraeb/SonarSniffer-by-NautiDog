#!/bin/bash
# CESAROPS Crate Publisher
# ========================
# Publishes a workspace crate to crates.io using the token from credentials.sh
#
# Usage:
#   bash scripts/publish_crate.sh nautivecs
#   bash scripts/publish_crate.sh nauticuvs
#   bash scripts/publish_crate.sh --dry-run nautivecs

set -euo pipefail

REPO="/home/cesarops/wreckhunter2000-1"
source "$REPO/scripts/credentials.sh" 2>/dev/null || source "$(dirname "$0")/credentials.sh"

# ── Parse args ─────────────────────────────────────────────────────────────────

DRY_RUN=""
CRATE=""

for arg in "$@"; do
    case "$arg" in
        --dry-run) DRY_RUN="--dry-run" ;;
        *) CRATE="$arg" ;;
    esac
done

if [ -z "$CRATE" ]; then
    echo "Usage: bash scripts/publish_crate.sh [--dry-run] <crate-name>"
    echo ""
    echo "Available crates:"
    echo "  nautivecs    — AST-aware context injection engine"
    echo "  nauticuvs    — Precision curvelet transform engine"
    exit 1
fi

if [ -z "${CRATES_IO_TOKEN:-}" ]; then
    echo "ERROR: CRATES_IO_TOKEN not set in credentials.sh"
    exit 1
fi

# ── Pre-flight checks ──────────────────────────────────────────────────────────

echo "╔══════════════════════════════════════════════════════════╗"
echo "║  CESAROPS Crate Publisher                                ║"
echo "╚══════════════════════════════════════════════════════════╝"
echo ""
echo "  Crate: $CRATE"
echo "  Mode:  ${DRY_RUN:-publish}"
echo ""

# Verify crate exists in workspace
if [ ! -f "$CRATE/Cargo.toml" ] && [ ! -f "$REPO/$CRATE/Cargo.toml" ]; then
    echo "ERROR: Crate directory '$CRATE' not found"
    exit 1
fi

# Run tests first
echo "[1/4] Running tests..."
cargo test -p "$CRATE" --quiet
echo "  ✓ Tests pass"

# Package check
echo "[2/4] Packaging..."
cargo package -p "$CRATE" --allow-dirty --quiet
echo "  ✓ Package valid"

# Login with token
echo "[3/4] Authenticating..."
cargo login "$CRATES_IO_TOKEN" 2>/dev/null
echo "  ✓ Logged in"

# Publish
echo "[4/4] Publishing${DRY_RUN:+ (dry run)}..."
if [ -n "$DRY_RUN" ]; then
    echo "  [DRY RUN] Would publish $CRATE — skipping actual upload"
else
    cargo publish -p "$CRATE" --allow-dirty
    echo "  ✓ Published to crates.io!"
    echo ""
    echo "  View at: https://crates.io/crates/$CRATE"
fi

echo ""
echo "Done."
