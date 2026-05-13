#!/bin/bash
# Benchmark: mistral.rs and candle on GTX 1070
# Focused test — no Crane, just the two Rust engines we care about
set -e
source "$HOME/.cargo/env" 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
GGUF_MODEL="$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf"
SAFETENSORS_DIR="$BENCH_DIR/models/qwen25-1.5b"
PROMPT="The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves"

pkill -f crane-oai 2>/dev/null || true
pkill -f mistralrs 2>/dev/null || true

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  BENCHMARK: mistral.rs + candle — GTX 1070 (8GB)            ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# Check CUDA
echo ""
echo "[CUDA check]"
if nvcc --version > /dev/null 2>&1; then
    echo "  nvcc: $(nvcc --version 2>&1 | grep release)"
else
    echo "  No nvcc — installing cuda toolkit..."
    echo cesarops | sudo -S apt-get install -y nvidia-cuda-toolkit 2>&1 | tail -3
fi

# ── Build candle quantized (GGUF support, CUDA) ──────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building candle (quantized example with CUDA)..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "candle" ]; then
    git clone --depth 1 https://github.com/huggingface/candle.git
fi
cd candle

# Try CUDA first, fall back to CPU
if nvcc --version > /dev/null 2>&1; then
    echo "  Building with CUDA..."
    cargo build --release --example quantized --features cuda 2>&1 | tail -5
else
    echo "  Building CPU-only..."
    cargo build --release --example quantized 2>&1 | tail -5
fi

CANDLE_BIN="./target/release/examples/quantized"
if [ ! -f "$CANDLE_BIN" ]; then
    echo "  CUDA build failed, trying without..."
    cargo build --release --example quantized 2>&1 | tail -5
fi

# ── Run candle benchmark ──────────────────────────────────────────────────────
if [ -f "$CANDLE_BIN" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST: candle quantized — TinyLlama 1.1B Q4_K_M"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    START=$(date +%s%N)
    OUTPUT=$($CANDLE_BIN --model "$GGUF_MODEL" --prompt "$PROMPT" --sample-len 128 2>&1)
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    
    # candle prints tok/s in its output
    REPORTED_TOKS=$(echo "$OUTPUT" | grep -oP '[\d.]+\s*token' | head -1 | grep -oP '[\d.]+' || true)
    CALC_TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
    
    echo ""
    echo "  ⏱  Total time: ${ELAPSED}ms"
    echo "  📊 Calculated tok/s: $CALC_TOKS"
    if [ -n "$REPORTED_TOKS" ]; then
        echo "  📊 Reported tok/s: $REPORTED_TOKS"
    fi
    echo "  📝 Output:"
    echo "$OUTPUT" | grep -v "^$" | tail -10 | head -5 | sed 's/^/     /'
    echo ""
    
    CANDLE_RESULT="candle: ${CALC_TOKS} tok/s (${ELAPSED}ms)"
else
    echo "  ✗ candle build failed entirely"
    CANDLE_RESULT="candle: BUILD FAILED"
fi

# ── Build mistral.rs ──────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building mistral.rs..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "mistralrs" ]; then
    git clone https://github.com/EricLBuehler/mistral.rs.git mistralrs
fi
cd mistralrs

# mistral.rs uses feature flags for CUDA
if nvcc --version > /dev/null 2>&1; then
    echo "  Building with CUDA..."
    cargo build --release --features cuda 2>&1 | tail -10
else
    echo "  Building CPU-only..."
    cargo build --release 2>&1 | tail -10
fi

MISTRAL_BIN="./target/release/mistralrs-server"
if [ ! -f "$MISTRAL_BIN" ]; then
    echo "  Full build failed, trying CPU fallback..."
    cargo build --release 2>&1 | tail -5
fi

# ── Run mistral.rs benchmark ──────────────────────────────────────────────────
if [ -f "$MISTRAL_BIN" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST: mistral.rs — Qwen2.5-1.5B (safetensors)"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    # mistral.rs is a server — start it, query it, kill it
    $MISTRAL_BIN --port 5557 plain -m "$SAFETENSORS_DIR" -a qwen2 2>&1 &
    MISTRAL_PID=$!
    
    echo "  Waiting for mistral.rs to load..."
    READY=0
    for i in $(seq 1 120); do
        if curl -s http://localhost:5557/v1/models > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            READY=1
            break
        fi
        if ! kill -0 $MISTRAL_PID 2>/dev/null; then
            echo "  ERROR: mistral.rs died"
            READY=0
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5557/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"qwen25\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}" \
            --max-time 120)
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        CALC_TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 Approx tok/s: $CALC_TOKS"
        echo "  📝 Response:"
        echo "$RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    text = r.get('choices',[{}])[0].get('text','')[:200]
    usage = r.get('usage',{})
    print(f'     {text}')
    if usage:
        print(f'     [usage: {usage}]')
except:
    print('     (parse error)')
" 2>/dev/null
        
        MISTRAL_RESULT="mistral.rs: ${CALC_TOKS} tok/s (${ELAPSED}ms)"
    else
        MISTRAL_RESULT="mistral.rs: FAILED TO START"
    fi
    
    kill $MISTRAL_PID 2>/dev/null || true
    wait $MISTRAL_PID 2>/dev/null || true
else
    echo "  ✗ mistral.rs build failed"
    MISTRAL_RESULT="mistral.rs: BUILD FAILED"
fi

# ── Final Summary ─────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  FINAL RESULTS                                               ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "  KoboldCPP (CUDA):   94.7 tok/s  [baseline]"
echo "  wgpu-llm (Vulkan):  26.0 tok/s  [portable]"
echo "  $CANDLE_RESULT"
echo "  $MISTRAL_RESULT"
echo ""
echo "  GPU: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1)"
echo "  CUDA: $(nvcc --version 2>&1 | grep release || echo 'not available')"
