#!/bin/bash
# Deploy cesarops-mcp-steered to T440 as the SCM controller
set -e

echo "=== Installing Rust toolchain ==="
if ! command -v cargo > /dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
fi
source "$HOME/.cargo/env"
cargo --version

echo ""
echo "=== Building cesarops-mcp-steered ==="
cd ~/wreckhunter2000-1
cargo build --release -p cesarops-mcp-steered 2>&1 | tail -5

echo ""
echo "=== Installing binary ==="
sudo cp target/release/cesarops-mcp /usr/local/bin/cesarops-mcp
sudo chmod +x /usr/local/bin/cesarops-mcp
echo "Installed: $(ls -lh /usr/local/bin/cesarops-mcp)"

echo ""
echo "=== Creating systemd service ==="
sudo tee /etc/systemd/system/cesarops-scm.service > /dev/null << 'EOF'
[Unit]
Description=CESARops SCM Controller (MCP Server)
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=cesarops
ExecStart=/usr/local/bin/cesarops-mcp
Environment="LLM_URL=http://100.102.158.111:5555/v1"
Environment="LLM_MODEL=qwen2.5-coder-3b-instruct"
Environment="LLM_API_KEY=local"
Environment="NAUTIVECS_DB=/home/cesarops/wreckhunter2000-1/data/nautivecs_store.json"
Environment="EMBEDDING_URL=http://100.102.158.111:5555/v1"
Environment="CONTEXT_BUDGET=4096"
Environment="N8N_WEBHOOK_URL=http://100.102.158.111:5678/webhook/scm-feedback"
Environment="RUST_LOG=info"
Restart=on-failure
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
echo "Service created (not started — MCP servers run via stdio, not as daemons)"

echo ""
echo "=== Testing binary ==="
cesarops-mcp --help 2>&1 | head -5

echo ""
echo "=== DONE ==="
echo "The SCM controller is ready."
echo "It will be invoked by Kiro via MCP stdio when configured in .kiro/settings/mcp.json"
echo ""
echo "To test manually:"
echo "  echo '{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/list\"}' | cesarops-mcp"
