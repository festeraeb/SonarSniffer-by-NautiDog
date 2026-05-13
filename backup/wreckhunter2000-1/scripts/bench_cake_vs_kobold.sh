#!/bin/bash
# Benchmark: Cake (Vulkan) vs KoboldCPP (CUDA) — same model, same prompt
# Model: Qwen2.5-Coder-3B-Instruct Q4_K_M
# Hardware: GTX 1070 (8GB) + P1000 (4GB)
set -e
source ~/.cargo/env 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
MODEL="$BENCH_DIR/models/qwen2.5-coder-3b-instruct-q4_k_m.gguf"
CAKE_BIN="$BENCH_DIR/cake/target/release/cake"
KOBOLD_BIN="$BENCH_DIR/koboldcpp"
PROMPT="The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves"
MAX_TOKENS=128

pkill -f koboldcpp 2>/dev/null || true
pkill -f cake 2>/dev/null || true
sleep 2

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  BENCHMARK: Cake (Vulkan) vs KoboldCPP (CUDA)              ║"
echo "║  Model: Qwen2.5-Coder-3B-Instruct Q4_K_M (2.0GB)          ║"
echo "║  Hardware: GTX 1070 (8GB) + P1000 (4GB)                    ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
nvidia-smi --query-gpu=index,name,memory.total,memory.free --format=csv,noheader
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# TEST 1: KoboldCPP (CUDA) — baseline
# ══════════════════════════════════════════════════════════════════════════════
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST 1: KoboldCPP (CUDA) — Qwen2.5-Coder-3B"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

$KOBOLD_BIN --model "$MODEL" --port 5555 --gpulayers 99 --contextsize 8192 --quiet &
KOBOLD_PID=$!

echo "  Loading model..."
for i in $(seq 1 90); do
    if curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1; then
        echo "  Ready after ${i}s"
        break
    fi
    sleep 1
done

if curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1; then
    # Warm-up run
    curl -s -X POST http://localhost:5555/api/v1/generate \
        -H "Content-Type: application/json" \
        -d "{\"prompt\": \"Hello\", \"max_length\": 10}" > /dev/null 2>&1

    # Timed run
    START=$(date +%s%N)
    RESP=$(curl -s -X POST http://localhost:5555/api/v1/generate \
        -H "Content-Type: application/json" \
        -d "{\"prompt\": \"$PROMPT\", \"max_length\": $MAX_TOKENS, \"temperature\": 0.7}")
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    TOKS=$(python3 -c "print(f'{$MAX_TOKENS / ($ELAPSED / 1000.0):.2f}')")

    TEXT=$(echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'][:150])
except: print('(error)')
" 2>/dev/null)

    echo ""
    echo "  ⏱  Time: ${ELAPSED}ms"
    echo "  📊 tok/s: $TOKS"
    echo "  📝 $TEXT"
    KOBOLD_RESULT="$TOKS"
else
    echo "  ✗ KoboldCPP failed to start"
    KOBOLD_RESULT="FAILED"
fi

kill $KOBOLD_PID 2>/dev/null || true
wait $KOBOLD_PID 2>/dev/null || true
sleep 3

# ══════════════════════════════════════════════════════════════════════════════
# TEST 2: Cake (Vulkan) — single node
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST 2: Cake (Vulkan) — Qwen2.5-Coder-3B"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Cake needs safetensors format — check if we have it, otherwise use GGUF mode
# Cake supports GGUF via its CLI directly
echo "  Starting Cake server..."
$CAKE_BIN serve --model-path "$MODEL" --port 5556 2>&1 &
CAKE_PID=$!

echo "  Loading model..."
for i in $(seq 1 120); do
    if curl -s http://localhost:5556/v1/models > /dev/null 2>&1; then
        echo "  Ready after ${i}s"
        break
    fi
    if ! kill -0 $CAKE_PID 2>/dev/null; then
        echo "  ✗ Cake died during startup"
        # Try alternative CLI
        echo "  Trying: cake run with direct generation..."
        START=$(date +%s%N)
        OUTPUT=$($CAKE_BIN run --model-path "$MODEL" "$PROMPT" --max-tokens $MAX_TOKENS 2>&1)
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{$MAX_TOKENS / ($ELAPSED / 1000.0):.2f}')")
        echo ""
        echo "  ⏱  Time: ${ELAPSED}ms"
        echo "  📊 tok/s: $TOKS"
        echo "  📝 $(echo "$OUTPUT" | tail -5 | head -3)"
        CAKE_RESULT="$TOKS"
        CAKE_PID=""
        break
    fi
    sleep 1
done

if [ -n "$CAKE_PID" ] && curl -s http://localhost:5556/v1/models > /dev/null 2>&1; then
    # Warm-up
    curl -s -X POST http://localhost:5556/v1/completions \
        -H "Content-Type: application/json" \
        -d "{\"model\":\"qwen\",\"prompt\":\"Hello\",\"max_tokens\":10}" > /dev/null 2>&1

    # Timed run
    START=$(date +%s%N)
    RESP=$(curl -s -X POST http://localhost:5556/v1/completions \
        -H "Content-Type: application/json" \
        -d "{\"model\":\"qwen\",\"prompt\":\"$PROMPT\",\"max_tokens\":$MAX_TOKENS,\"temperature\":0.7}" \
        --max-time 60)
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    TOKS=$(python3 -c "print(f'{$MAX_TOKENS / ($ELAPSED / 1000.0):.2f}')")

    TEXT=$(echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r.get('choices',[{}])[0].get('text','')[:150])
except: print('(error)')
" 2>/dev/null)

    echo ""
    echo "  ⏱  Time: ${ELAPSED}ms"
    echo "  📊 tok/s: $TOKS"
    echo "  📝 $TEXT"
    CAKE_RESULT="$TOKS"
elif [ -z "$CAKE_RESULT" ]; then
    CAKE_RESULT="FAILED"
fi

[ -n "$CAKE_PID" ] && kill $CAKE_PID 2>/dev/null || true
wait $CAKE_PID 2>/dev/null || true

# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  RESULTS: Qwen2.5-Coder-3B on GTX 1070                     ║"
echo "╠══════════════════════════════════════════════════════════════╣"
printf "║  KoboldCPP (CUDA):  %-10s tok/s                       ║\n" "$KOBOLD_RESULT"
printf "║  Cake (Vulkan):     %-10s tok/s                       ║\n" "$CAKE_RESULT"
echo "║                                                              ║"
echo "║  Previous (TinyLlama 1.1B):                                 ║"
echo "║  KoboldCPP: 94.7 tok/s | wgpu-llm: 26.0 tok/s              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
