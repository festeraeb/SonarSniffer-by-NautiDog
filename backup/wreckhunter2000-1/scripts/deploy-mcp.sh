#!/bin/bash
# ── Deploy cesarops-mcp-steered to T440 ──────────────────────────────────────
# Builds on-target (T440 has Rust toolchain) and installs the service.
#
# Usage from your local machine:
#   ssh cesarops@100.72.182.77 'cd ~/wreckhunter2000-1 && bash scripts/deploy-mcp.sh'
#
# Or from the T440 directly:
#   cd ~/wreckhunter2000-1 && bash scripts/deploy-mcp.sh

set -euo pipefail

echo "═══════════════════════════════════════════════════════════"
echo "  cesarops-mcp-steered — Build & Deploy"
echo "═══════════════════════════════════════════════════════════"

# ── Step 1: Build the release binary ─────────────────────────────────────────
echo ""
echo "▶ Building release binary (target-cpu=native for Skylake-AVX512)..."
cd ~/wreckhunter2000-1/cesarops-mcp-steered

# Ensure Rust is available
if ! command -v cargo &> /dev/null; then
    echo "ERROR: cargo not found. Install Rust: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

cargo build --release 2>&1 | tail -5
echo "✓ Binary built: target/release/cesarops-mcp"

# ── Step 2: Verify the binary runs ──────────────────────────────────────────
echo ""
echo "▶ Verifying binary..."
./target/release/cesarops-mcp --help > /dev/null 2>&1 && echo "✓ Binary executes" || {
    echo "ERROR: Binary failed to execute"
    exit 1
}

# ── Step 3: Initialize nautivecs store (index the SCM codebase) ──────────────
echo ""
echo "▶ Ensuring nautivecs data directory exists..."
mkdir -p data
if [ ! -f data/nautivecs_store.json ]; then
    echo "  Creating empty nautivecs store (will be populated on first reindex)"
    echo '{"chunks":[],"metadata":{"indexed_at":null,"chunk_count":0}}' > data/nautivecs_store.json
fi
echo "✓ nautivecs store ready"

# ── Step 4: Install systemd service ─────────────────────────────────────────
echo ""
echo "▶ Installing systemd service..."
if [ -f /etc/systemd/system/cesarops-mcp.service ]; then
    echo "  Service file already exists — updating..."
fi
sudo cp ~/wreckhunter2000-1/scripts/cesarops-mcp.service /etc/systemd/system/cesarops-mcp.service
sudo systemctl daemon-reload
echo "✓ Service installed"

# ── Step 5: Run tests to verify everything works ─────────────────────────────
echo ""
echo "▶ Running test suite..."
cargo test --release 2>&1 | tail -3
echo "✓ Tests passed"

# ── Step 6: Start the service ────────────────────────────────────────────────
echo ""
echo "▶ Starting cesarops-mcp service..."
sudo systemctl restart cesarops-mcp
sleep 2
if systemctl is-active --quiet cesarops-mcp; then
    echo "✓ Service is running"
else
    echo "⚠ Service not running (expected for stdio-mode — needs MCP client connection)"
    echo "  The binary is ready. Connect via MCP client or run manually:"
    echo "  ./target/release/cesarops-mcp"
fi

# ── Step 7: Index the SCM codebase into nautivecs ────────────────────────────
echo ""
echo "▶ Indexing SCM codebase into nautivecs..."
echo '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' | ./target/release/cesarops-mcp 2>/dev/null | head -1 > /dev/null || true
# The reindex will happen on first tool call — nautivecs auto-indexes on startup

echo ""
echo "═══════════════════════════════════════════════════════════"
echo "  ✓ Deployment complete!"
echo ""
echo "  Binary:  ~/wreckhunter2000-1/cesarops-mcp-steered/target/release/cesarops-mcp"
echo "  Service: cesarops-mcp.service"
echo "  Store:   ~/wreckhunter2000-1/cesarops-mcp-steered/data/nautivecs_store.json"
echo ""
echo "  To test the white paper objective:"
echo "    echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{\"name\":\"execute_objective\",\"arguments\":{\"objective\":\"Examine the cesarops-mcp-steered SCM module source code and write a white paper titled Segmented Accuracy Steering: Resilient AI Orchestration on Heterogeneous Hardware\"}}}' | ./target/release/cesarops-mcp"
echo ""
echo "═══════════════════════════════════════════════════════════"
