#!/bin/bash
set -e
source ~/.cargo/env 2>/dev/null || true
CAKE=~/benchmark/cake/target/release/cake
KOBOLD=~/benchmark/koboldcpp
MODEL_GGUF=~/benchmark/models/qwen2.5-coder-3b-instruct-q4_k_m.gguf
PROMPT="The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves"

pkill -f koboldcpp 2>/dev/null || true
pkill -f cake 2>/dev/null || true
sleep 2

echo "=== PULLING MODEL VIA CAKE ==="
$CAKE pull Qwen/Qwen2.5-Coder-3B-Instruct 2>&1 | tail -5

echo ""
echo "=== CAKE RUN (Vulkan, Qwen2.5-Coder-3B) ==="
START=$(date +%s%N)
$CAKE run Qwen/Qwen2.5-Coder-3B-Instruct "$PROMPT" --sample-len 128 --temperature 0.7 2>&1 | tee /tmp/cake_out.txt
END=$(date +%s%N)
CAKE_MS=$(( (END - START) / 1000000 ))
echo ""
echo "CAKE_TIME: ${CAKE_MS}ms"
# Extract tok/s from cake output if available
grep -oP '[\d.]+\s*tok' /tmp/cake_out.txt | head -1 || true

echo ""
echo "=== KOBOLDCPP (CUDA, Qwen2.5-Coder-3B Q4_K_M) ==="
$KOBOLD --model "$MODEL_GGUF" --port 5555 --gpulayers 99 --contextsize 8192 --quiet &
KPID=$!
for i in $(seq 1 60); do
    curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1 && break
    sleep 1
done

if curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1; then
    START2=$(date +%s%N)
    curl -s -X POST http://localhost:5555/api/v1/generate \
        -H "Content-Type: application/json" \
        -d "{\"prompt\": \"$PROMPT\", \"max_length\": 128, \"temperature\": 0.7}" > /tmp/kobold_out.txt
    END2=$(date +%s%N)
    KOBOLD_MS=$(( (END2 - START2) / 1000000 ))
    echo "KOBOLD_TIME: ${KOBOLD_MS}ms"
    KOBOLD_TPS=$(python3 -c "print(f'{128 / ($KOBOLD_MS / 1000.0):.1f}')")
    echo "KOBOLD_TPS: $KOBOLD_TPS tok/s"
else
    echo "KOBOLD FAILED TO START"
    KOBOLD_MS=0
    KOBOLD_TPS=0
fi
kill $KPID 2>/dev/null || true

echo ""
echo "=== FINAL COMPARISON ==="
CAKE_TPS=$(python3 -c "print(f'{128 / ($CAKE_MS / 1000.0):.1f}')" 2>/dev/null || echo "?")
echo "Cake (Vulkan):    ${CAKE_MS}ms = ${CAKE_TPS} tok/s"
echo "KoboldCPP (CUDA): ${KOBOLD_MS}ms = ${KOBOLD_TPS} tok/s"
echo ""
echo "Previous baseline (TinyLlama 1.1B):"
echo "  KoboldCPP: 94.7 tok/s"
echo "  wgpu-llm:  26.0 tok/s"
