#!/bin/bash
source ~/.cargo/env 2>/dev/null || true
pkill -f cake 2>/dev/null; sleep 2
CAKE=~/benchmark/cake/target/release/cake
MODEL=~/benchmark/models/qwen25-1.5b
echo "=== CAKE 1.5B BENCHMARK ===" > /tmp/cake_bench.log
echo "Model: $MODEL" >> /tmp/cake_bench.log
$CAKE run --model "$MODEL" --text-model-arch qwen2 \
  "The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves" \
  --sample-len 128 --temperature 0.7 >> /tmp/cake_bench.log 2>&1
echo "=== DONE ===" >> /tmp/cake_bench.log
