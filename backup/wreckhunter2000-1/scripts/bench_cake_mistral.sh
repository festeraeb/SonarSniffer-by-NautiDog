#!/bin/bash
# Benchmark: Cake (Vulkan distributed) + mistral.rs vs KoboldCPP baseline
# Test 1: TinyLlama 1.1B single-GPU (apples-to-apples with KoboldCPP 94.7 tok/s)
# Test 2: Large model (Qwen3-8B) distributed across 1070+P1000 via Cake
set -e
source "$HOME/.cargo/env" 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
GGUF_MODEL="$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf"
PROMPT="The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves"

pkill -f cake 2>/dev/null || true
pkill -f mistralrs 2>/dev/null || true
pkill -f koboldcpp 2>/dev/null || true
sleep 2

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  BENCHMARK: Cake + mistral.rs — GTX 1070 + P1000            ║"
echo "║  Test 1: Small model single-GPU (vs KoboldCPP 94.7 tok/s)  ║"
echo "║  Test 2: Large model distributed across both GPUs           ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "GPUs:"
nvidia-smi --query-gpu=index,name,memory.total --format=csv,noheader
echo ""

# ── Install Cake ──────────────────────────────────────────────────────────────
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building Cake (Vulkan backend)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "cake" ]; then
    git clone https://github.com/evilsocket/cake.git
fi
cd cake

# Build with Vulkan (portable, uses our wgpu/Vulkan stack)
cargo build --release --features vulkan 2>&1 | tail -10
CAKE_BIN="./target/release/cake"

if [ ! -f "$CAKE_BIN" ]; then
    echo "  Vulkan build failed, trying CUDA..."
    cargo build --release --features cuda 2>&1 | tail -10
fi

if [ ! -f "$CAKE_BIN" ]; then
    echo "  ✗ Cake build failed entirely"
    CAKE_BIN=""
else
    echo "  ✓ Cake built: $(ls -lh $CAKE_BIN | awk '{print $5}')"
fi

# ── Install mistral.rs ────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building mistral.rs (CUDA backend)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "mistralrs" ]; then
    git clone https://github.com/EricLBuehler/mistral.rs.git mistralrs
fi
cd mistralrs

# Try CUDA first (fastest), fall back to CPU
if nvcc --version > /dev/null 2>&1; then
    cargo build --release --features cuda 2>&1 | tail -10
else
    cargo build --release 2>&1 | tail -10
fi

MISTRAL_BIN="./target/release/mistralrs-server"
if [ ! -f "$MISTRAL_BIN" ]; then
    echo "  CUDA failed, trying CPU..."
    cargo build --release 2>&1 | tail -5
fi

if [ -f "$MISTRAL_BIN" ]; then
    echo "  ✓ mistral.rs built: $(ls -lh $MISTRAL_BIN | awk '{print $5}')"
else
    echo "  ✗ mistral.rs build failed"
    MISTRAL_BIN=""
fi

# ══════════════════════════════════════════════════════════════════════════════
# TEST 1: Small model single-GPU — apples-to-apples with KoboldCPP
# ══════════════════════════════════════════════════════════════════════════════

echo ""
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  TEST 1: TinyLlama 1.1B — Single GPU (GTX 1070)            ║"
echo "║  Baseline: KoboldCPP = 94.7 tok/s                          ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# ── Cake single-node (TinyLlama) ─────────────────────────────────────────────
if [ -n "$CAKE_BIN" ]; then
    echo ""
    echo "  [Cake — Vulkan single-node]"
    
    # Pull TinyLlama via Cake's model system
    echo "  Pulling model..."
    $CAKE_BIN pull TinyLlama/TinyLlama-1.1B-Chat-v1.0 2>&1 | tail -3 || true
    
    # Start Cake server
    $CAKE_BIN serve TinyLlama/TinyLlama-1.1B-Chat-v1.0 --port 5558 2>&1 &
    CAKE_PID=$!
    
    echo "  Waiting for Cake to load..."
    READY=0
    for i in $(seq 1 90); do
        if curl -s http://localhost:5558/v1/models > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            READY=1
            break
        fi
        if ! kill -0 $CAKE_PID 2>/dev/null; then
            echo "  ERROR: Cake died"
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5558/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"TinyLlama\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}" \
            --max-time 60)
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 tok/s: $TOKS"
        echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    text = r.get('choices',[{}])[0].get('text','')[:150]
    print(f'  📝 {text}')
except: print('  📝 (parse error)')
" 2>/dev/null
        CAKE_SMALL="$TOKS"
    else
        CAKE_SMALL="FAILED"
    fi
    
    kill $CAKE_PID 2>/dev/null || true
    wait $CAKE_PID 2>/dev/null || true
    sleep 2
else
    CAKE_SMALL="NOT BUILT"
fi

# ── mistral.rs single-node (TinyLlama GGUF) ──────────────────────────────────
if [ -n "$MISTRAL_BIN" ]; then
    echo ""
    echo "  [mistral.rs — CUDA single-node]"
    
    $MISTRAL_BIN --port 5559 gguf -m "$GGUF_MODEL" -t tinyllama 2>&1 &
    MISTRAL_PID=$!
    
    echo "  Waiting for mistral.rs to load..."
    READY=0
    for i in $(seq 1 90); do
        if curl -s http://localhost:5559/v1/models > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            READY=1
            break
        fi
        if ! kill -0 $MISTRAL_PID 2>/dev/null; then
            echo "  ERROR: mistral.rs died"
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5559/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"tinyllama\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}" \
            --max-time 60)
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 tok/s: $TOKS"
        echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    text = r.get('choices',[{}])[0].get('text','')[:150]
    print(f'  📝 {text}')
except: print('  📝 (parse error)')
" 2>/dev/null
        MISTRAL_SMALL="$TOKS"
    else
        MISTRAL_SMALL="FAILED"
    fi
    
    kill $MISTRAL_PID 2>/dev/null || true
    wait $MISTRAL_PID 2>/dev/null || true
    sleep 2
else
    MISTRAL_SMALL="NOT BUILT"
fi

# ══════════════════════════════════════════════════════════════════════════════
# TEST 2: Large model distributed — Qwen3-8B across 1070 + P1000
# This is the real test: can we run a model too big for one GPU?
# 1070 (8GB) + P1000 (4GB) = 12GB available
# Qwen3-8B Q4 ≈ 5GB — fits on 1070 alone but let's test distributed
# Qwen3-14B Q4 ≈ 8.5GB — needs both GPUs
# ══════════════════════════════════════════════════════════════════════════════

echo ""
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  TEST 2: Large Model Distributed — Qwen3-8B across GPUs    ║"
echo "║  GTX 1070 (8GB) + P1000 (4GB) = 12GB cluster               ║"
echo "╚══════════════════════════════════════════════════════════════╝"

if [ -n "$CAKE_BIN" ]; then
    echo ""
    echo "  [Cake — Vulkan distributed across both GPUs]"
    
    # Pull Qwen3-8B (Cake handles download)
    echo "  Pulling Qwen3-8B model..."
    $CAKE_BIN pull evilsocket/Qwen3-8B 2>&1 | tail -5 || \
    $CAKE_BIN pull Qwen/Qwen3-8B 2>&1 | tail -5 || true
    
    # Start Cake with distributed mode (auto-shards across available GPUs)
    echo "  Starting distributed inference..."
    $CAKE_BIN serve evilsocket/Qwen3-8B --port 5560 2>&1 &
    CAKE_PID=$!
    
    echo "  Waiting for Cake to load (large model, may take a minute)..."
    READY=0
    for i in $(seq 1 180); do
        if curl -s http://localhost:5560/v1/models > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            READY=1
            break
        fi
        if ! kill -0 $CAKE_PID 2>/dev/null; then
            echo "  ERROR: Cake died (model may be too large)"
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5560/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"Qwen3-8B\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}" \
            --max-time 120)
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 tok/s: $TOKS"
        echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    text = r.get('choices',[{}])[0].get('text','')[:200]
    print(f'  📝 {text}')
    usage = r.get('usage',{})
    if usage: print(f'  📊 usage: {usage}')
except: print('  📝 (parse error)')
" 2>/dev/null
        CAKE_LARGE="$TOKS"
    else
        CAKE_LARGE="FAILED"
    fi
    
    kill $CAKE_PID 2>/dev/null || true
    wait $CAKE_PID 2>/dev/null || true
    sleep 2
else
    CAKE_LARGE="NOT BUILT"
fi

# ══════════════════════════════════════════════════════════════════════════════
# FINAL RESULTS
# ══════════════════════════════════════════════════════════════════════════════

echo ""
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  FINAL RESULTS                                               ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║                                                              ║"
echo "║  TEST 1: TinyLlama 1.1B (single GPU, GTX 1070)             ║"
echo "║  ─────────────────────────────────────────────────────────  ║"
echo "║  KoboldCPP (CUDA):     94.7 tok/s  [baseline]              ║"
echo "║  wgpu-llm (Vulkan):    26.0 tok/s  [portable]              ║"
printf "║  Cake (Vulkan):        %-10s                           ║\n" "$CAKE_SMALL tok/s"
printf "║  mistral.rs (CUDA):   %-10s                           ║\n" "$MISTRAL_SMALL tok/s"
echo "║                                                              ║"
echo "║  TEST 2: Qwen3-8B (distributed, 1070+P1000)                ║"
echo "║  ─────────────────────────────────────────────────────────  ║"
printf "║  Cake (distributed):  %-10s                           ║\n" "$CAKE_LARGE tok/s"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "Key question: Does a larger distributed model produce better"
echo "threshold decisions than a fast small model with vector injection?"
echo ""
echo "GPU memory after tests:"
nvidia-smi --query-gpu=index,name,memory.used,memory.total --format=csv,noheader
