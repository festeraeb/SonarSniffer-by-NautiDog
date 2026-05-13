#!/usr/bin/env bash
# Switch KoboldCPP preset on the T440 or i7 node.
#
# Usage:
#   bash scripts/switch_kobold_preset.sh dual-32b      # Dual P100, Qwen 32B
#   bash scripts/switch_kobold_preset.sh single-14b    # Single P100, Qwen 14B + Phi-3 speculative
#   bash scripts/switch_kobold_preset.sh rust-expert   # GTX 1070, Fortytwo Rust-Coder (run on i7)
#   bash scripts/switch_kobold_preset.sh status        # Show current preset and status

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PRESET="${1:-status}"

case "$PRESET" in
    dual-32b)
        SERVICE="$SCRIPT_DIR/koboldcpp-dual-p100-32b.service"
        NAME="Dual P100 — Qwen2.5-Coder-32B"
        MODEL="/home/cesarops/models/qwen2.5-coder-32b-instruct-q4_k_m.gguf"
        ;;
    single-14b)
        SERVICE="$SCRIPT_DIR/koboldcpp-single-p100-14b.service"
        NAME="Single P100 — Qwen2.5-14B + Phi-3 speculative"
        MODEL="/home/cesarops/models/qwen2.5-coder-14b-instruct-q4_k_m.gguf"
        ;;
    rust-expert)
        SERVICE="$SCRIPT_DIR/koboldcpp-1070-rust-expert.service"
        NAME="GTX 1070 — Fortytwo Rust-Coder-14B Q6"
        MODEL="/mnt/data-external/cesarops/models/Fortytwo_Strand-Rust-Coder-14B-v1-Q6_K.gguf"
        ;;
    status)
        echo "── KoboldCPP Status ──────────────────────────────────────"
        systemctl status koboldcpp --no-pager -l 2>/dev/null || echo "koboldcpp: not running"
        systemctl status koboldcpp-rust --no-pager -l 2>/dev/null || echo "koboldcpp-rust: not running"
        echo ""
        echo "── Active model (port 5001) ──────────────────────────────"
        curl -s http://localhost:5001/api/v1/model 2>/dev/null || echo "not responding"
        echo ""
        echo "── Active model (port 5002) ──────────────────────────────"
        curl -s http://localhost:5002/api/v1/model 2>/dev/null || echo "not responding"
        exit 0
        ;;
    *)
        echo "Usage: $0 [dual-32b|single-14b|rust-expert|status]"
        exit 1
        ;;
esac

# Check model file exists
if [ ! -f "$MODEL" ]; then
    echo "✗ Model not found: $MODEL"
    if [ "$PRESET" = "dual-32b" ]; then
        echo "  Run: bash scripts/download_32b_t440.sh"
    fi
    exit 1
fi

echo "Switching to: $NAME"
echo ""

# Install and restart
sudo cp "$SERVICE" /etc/systemd/system/koboldcpp.service
sudo systemctl daemon-reload
sudo systemctl enable koboldcpp
sudo systemctl restart koboldcpp

echo "Waiting for KoboldCPP to load model..."
for i in $(seq 1 24); do
    sleep 5
    RESULT=$(curl -s http://localhost:5001/api/v1/model 2>/dev/null)
    if echo "$RESULT" | grep -q "result"; then
        echo ""
        echo "✓ KoboldCPP is up: $RESULT"
        echo ""
        echo "  Inference URL: http://$(hostname -I | awk '{print $1}'):5001/v1"
        echo "  Use in Copilot/Roo Code settings."
        exit 0
    fi
    echo -n "."
done

echo ""
echo "⚠ Still loading — check: journalctl -u koboldcpp -f"
