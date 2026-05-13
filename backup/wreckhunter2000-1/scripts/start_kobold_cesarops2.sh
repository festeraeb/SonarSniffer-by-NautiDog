#!/bin/bash
pkill -f koboldcpp 2>/dev/null
sleep 2
nohup ~/benchmark/koboldcpp \
  --model ~/benchmark/models/qwen2.5-coder-3b-instruct-q4_k_m.gguf \
  --port 5555 \
  --gpulayers 99 \
  --contextsize 8192 \
  --quiet > /tmp/kobold_3b.log 2>&1 &
echo "KoboldCPP started PID: $!"
