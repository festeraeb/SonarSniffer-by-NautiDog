#!/bin/bash
source ~/.cargo/env 2>/dev/null
echo "=== Cake build check ==="
cd ~/benchmark/cake 2>/dev/null && cargo build --release --features vulkan 2>&1 | tail -20 || echo "Cake dir not found"
echo ""
echo "=== mistral.rs build check ==="
cd ~/benchmark/mistralrs 2>/dev/null && cargo build --release --features cuda 2>&1 | tail -20 || echo "mistralrs dir not found"
echo ""
echo "=== Binary check ==="
ls -lh ~/benchmark/cake/target/release/cake 2>/dev/null || echo "No cake binary"
ls -lh ~/benchmark/mistralrs/target/release/mistralrs-server 2>/dev/null || echo "No mistralrs binary"
