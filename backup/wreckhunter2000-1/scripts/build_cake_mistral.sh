#!/bin/bash
# Build Cake (Vulkan) and mistral.rs (CUDA) on cesarops2
set -e
source ~/.cargo/env

BENCH_DIR="$HOME/benchmark"
cd "$BENCH_DIR"

echo "=== Building Cake (Vulkan) ==="
if [ ! -d "cake" ]; then
    git clone https://github.com/evilsocket/cake.git
fi
cd cake
echo "  Building with Vulkan feature..."
cargo build --release --features vulkan 2>&1 | tail -10
if [ -f "./target/release/cake" ]; then
    echo "  ✓ Cake built: $(ls -lh ./target/release/cake | awk '{print $5}')"
else
    echo "  ✗ Cake Vulkan failed, trying without features..."
    cargo build --release 2>&1 | tail -10
    ls -lh ./target/release/cake 2>/dev/null && echo "  ✓ Cake (CPU) built" || echo "  ✗ Cake failed"
fi

echo ""
echo "=== Building mistral.rs ==="
cd "$BENCH_DIR/mistralrs"
echo "  Building with CUDA..."
cargo build --release --features cuda 2>&1 | tail -10
if [ -f "./target/release/mistralrs-server" ]; then
    echo "  ✓ mistral.rs built: $(ls -lh ./target/release/mistralrs-server | awk '{print $5}')"
else
    echo "  CUDA failed, trying CPU..."
    cargo build --release 2>&1 | tail -10
    ls -lh ./target/release/mistralrs-server 2>/dev/null && echo "  ✓ mistral.rs (CPU) built" || echo "  ✗ mistral.rs failed"
fi

echo ""
echo "=== Summary ==="
ls -lh "$BENCH_DIR/cake/target/release/cake" 2>/dev/null || echo "No cake binary"
ls -lh "$BENCH_DIR/mistralrs/target/release/mistralrs-server" 2>/dev/null || echo "No mistralrs binary"
