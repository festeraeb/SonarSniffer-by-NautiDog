#!/bin/bash
source /home/cesarops/.cargo/env
CAKE=/home/cesarops/benchmark/cake/target/release/cake

echo "=== Testing Cake with --dtype bf16 ==="
$CAKE run Qwen/Qwen2.5-1.5B-Instruct "Shipwreck-detection" --dtype bf16 --sample-len 64 --temperature 0.7 2>&1 | grep -E "INFO|token|Error"

echo ""
echo "=== Testing Cake with --dtype f32 ==="
$CAKE run Qwen/Qwen2.5-1.5B-Instruct "Shipwreck-detection" --dtype f32 --sample-len 64 --temperature 0.7 2>&1 | grep -E "INFO|token|Error"

echo ""
echo "=== Checking available evilsocket models ==="
$CAKE pull evilsocket/Qwen2.5-Coder-1.5B-Instruct 2>&1 | head -5
echo "DONE"
