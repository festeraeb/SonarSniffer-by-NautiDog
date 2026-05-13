#!/bin/bash
# Setup benchmark environment on cesarops2
# Downloads KoboldCPP, builds wgpu-llm, downloads TinyLlama safetensors
set -e

BENCH_DIR="$HOME/benchmark"
MODEL_DIR="$BENCH_DIR/models"
mkdir -p "$MODEL_DIR"

source "$HOME/.cargo/env" 2>/dev/null || true

echo "=== Step 1: Download KoboldCPP ==="
cd "$BENCH_DIR"
# Get the actual download URL from GitHub API
KOBOLD_URL=$(curl -sL "https://api.github.com/repos/LostRuins/koboldcpp/releases/latest" \
  | python3 -c "
import sys, json
r = json.load(sys.stdin)
for a in r.get('assets', []):
    if 'linux' in a['name'].lower() and 'cuda' in a['name'].lower() and 'nocuda' not in a['name'].lower():
        print(a['browser_download_url'])
        break
")

if [ -n "$KOBOLD_URL" ]; then
    echo "Downloading: $KOBOLD_URL"
    curl -L -o koboldcpp "$KOBOLD_URL"
    chmod +x koboldcpp
    echo "KoboldCPP: $(ls -lh koboldcpp | awk '{print $5}')"
else
    echo "WARNING: Could not find KoboldCPP CUDA binary in latest release"
    # Fallback: try nocuda
    KOBOLD_URL=$(curl -sL "https://api.github.com/repos/LostRuins/koboldcpp/releases/latest" \
      | python3 -c "
import sys, json
r = json.load(sys.stdin)
for a in r.get('assets', []):
    if 'linux' in a['name'].lower() and 'nocuda' not in a['name'].lower():
        print(a['browser_download_url'])
        break
")
    if [ -n "$KOBOLD_URL" ]; then
        echo "Downloading fallback: $KOBOLD_URL"
        curl -L -o koboldcpp "$KOBOLD_URL"
        chmod +x koboldcpp
    fi
fi

echo ""
echo "=== Step 2: Build wgpu-llm ==="
cd "$BENCH_DIR"
if [ ! -d "wgpu-llm" ]; then
    git clone https://github.com/Beledarian/wgpu-llm.git
fi
cd wgpu-llm
cargo build --release 2>&1 | tail -5
echo "wgpu-llm built: $(ls -lh target/release/wgpu-llm 2>/dev/null || echo 'build may have failed')"

echo ""
echo "=== Step 3: Download TinyLlama safetensors (for wgpu-llm) ==="
cd "$MODEL_DIR"
if [ ! -d "tinyllama-safetensors" ]; then
    mkdir -p tinyllama-safetensors
    cd tinyllama-safetensors
    # Download config, tokenizer, and model weights
    curl -L -o config.json "https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/config.json"
    curl -L -o tokenizer.json "https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/tokenizer.json"
    curl -L -o model.safetensors "https://huggingface.co/TinyLlama/TinyLlama-1.1B-Chat-v1.0/resolve/main/model.safetensors"
    echo "Safetensors model: $(ls -lh model.safetensors)"
else
    echo "Already downloaded"
fi

echo ""
echo "=== Step 4: Build Crane ==="
cd "$BENCH_DIR"
if [ ! -d "Crane" ]; then
    git clone https://github.com/lucasjinreal/Crane.git
fi
cd Crane
cargo build --release 2>&1 | tail -5
echo "Crane built: $(ls target/release/crane* 2>/dev/null || echo 'build may have failed')"

echo ""
echo "=== Setup Complete ==="
echo "Models:"
ls -lh "$MODEL_DIR"/tinyllama-1.1b-chat.Q4_K_M.gguf 2>/dev/null
ls -lh "$MODEL_DIR"/tinyllama-safetensors/model.safetensors 2>/dev/null
echo ""
echo "Binaries:"
ls -lh "$BENCH_DIR/koboldcpp" 2>/dev/null
ls -lh "$BENCH_DIR/wgpu-llm/target/release/wgpu-llm" 2>/dev/null
ls -lh "$BENCH_DIR/Crane/target/release/crane"* 2>/dev/null
