#!/bin/bash
# Full LLM inference benchmark: Crane (CUDA), mistral.rs, candle examples
# Run on cesarops2 after setup_benchmark.sh
set -e
source "$HOME/.cargo/env" 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
MODEL_DIR="$BENCH_DIR/models/qwen25-1.5b"
PROMPT="The Great Lakes region contains numerous shipwrecks. The most effective method for detecting submerged wrecks using satellite imagery involves"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  FULL LLM BENCHMARK — Crane CUDA + mistral.rs + candle     ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# Kill any leftover processes
pkill -f crane-oai 2>/dev/null || true
sleep 1

# ── Check CUDA toolkit ────────────────────────────────────────────────────────
echo ""
echo "[CHECK] CUDA toolkit..."
if nvcc --version > /dev/null 2>&1; then
    echo "  nvcc found: $(nvcc --version | grep release)"
else
    echo "  nvcc NOT found — installing CUDA toolkit..."
    echo cesarops | sudo -S apt-get install -y nvidia-cuda-toolkit 2>&1 | tail -3
    if nvcc --version > /dev/null 2>&1; then
        echo "  nvcc installed: $(nvcc --version | grep release)"
    else
        echo "  WARNING: CUDA toolkit install failed. Crane CUDA build will fail."
    fi
fi

# ── Rebuild Crane with CUDA ───────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building Crane with CUDA feature..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR/Crane"
cargo build --release --features cuda 2>&1 | tail -10
CRANE_BIN="./target/release/crane-oai"
if [ -f "$CRANE_BIN" ]; then
    echo "  ✓ Crane CUDA build complete"
else
    echo "  ✗ Crane CUDA build failed — will skip"
    CRANE_BIN=""
fi

# ── Test Crane with CUDA ──────────────────────────────────────────────────────
if [ -n "$CRANE_BIN" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST: Crane (Candle + CUDA) — Qwen2.5-1.5B"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    $CRANE_BIN --model-path "$MODEL_DIR" --port 5556 --format safetensors --model-type qwen25 2>&1 &
    CRANE_PID=$!
    
    echo "  Waiting for Crane to load..."
    READY=0
    for i in $(seq 1 90); do
        if curl -s http://localhost:5556/v1/models > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            READY=1
            break
        fi
        if ! kill -0 $CRANE_PID 2>/dev/null; then
            echo "  ERROR: Crane died during startup"
            wait $CRANE_PID 2>/dev/null || true
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5556/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"qwen25\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}")
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 Approx tok/s: $TOKS"
        echo "  📝 Response:"
        echo "$RESP" | python3 -c "import sys,json; r=json.load(sys.stdin); print('    ', r.get('choices',[{}])[0].get('text','')[:200])" 2>/dev/null || echo "    (parse error)"
    fi
    
    kill $CRANE_PID 2>/dev/null || true
    wait $CRANE_PID 2>/dev/null || true
    sleep 2
fi

# ── Clone and build mistral.rs ────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building mistral.rs..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "mistralrs" ]; then
    git clone https://github.com/EricLBuehler/mistral.rs.git mistralrs 2>&1 | tail -3
fi
cd mistralrs
# Build with CUDA support
cargo build --release --features cuda 2>&1 | tail -10
MISTRAL_BIN="./target/release/mistralrs-server"
if [ -f "$MISTRAL_BIN" ]; then
    echo "  ✓ mistral.rs CUDA build complete"
else
    # Try without CUDA
    echo "  CUDA build failed, trying CPU..."
    cargo build --release 2>&1 | tail -5
    MISTRAL_BIN="./target/release/mistralrs-server"
fi

# ── Test mistral.rs ───────────────────────────────────────────────────────────
if [ -f "$MISTRAL_BIN" ]; then
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST: mistral.rs — Qwen2.5-1.5B"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    $MISTRAL_BIN --port 5557 plain -m "$MODEL_DIR" 2>&1 &
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
            echo "  ERROR: mistral.rs died during startup"
            wait $MISTRAL_PID 2>/dev/null || true
            break
        fi
        sleep 1
    done
    
    if [ $READY -eq 1 ]; then
        echo "  Running inference (128 tokens)..."
        START=$(date +%s%N)
        
        RESP=$(curl -s -X POST http://localhost:5557/v1/completions \
            -H "Content-Type: application/json" \
            -d "{\"model\":\"qwen25\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}")
        
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED}ms"
        echo "  📊 Approx tok/s: $TOKS"
        echo "  📝 Response:"
        echo "$RESP" | python3 -c "import sys,json; r=json.load(sys.stdin); print('    ', r.get('choices',[{}])[0].get('text','')[:200])" 2>/dev/null || echo "    (parse error)"
    fi
    
    kill $MISTRAL_PID 2>/dev/null || true
    wait $MISTRAL_PID 2>/dev/null || true
    sleep 2
else
    echo "  ✗ mistral.rs build failed entirely"
fi

# ── Candle example (direct, no server) ────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Building candle-transformers example..."
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cd "$BENCH_DIR"
if [ ! -d "candle" ]; then
    git clone --depth 1 https://github.com/huggingface/candle.git 2>&1 | tail -3
fi
cd candle
# Build the quantized example which supports GGUF
cargo build --release --example quantized --features cuda 2>&1 | tail -5
CANDLE_BIN="./target/release/examples/quantized"
if [ -f "$CANDLE_BIN" ]; then
    echo "  ✓ candle quantized example built"
    
    echo ""
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "  TEST: candle (quantized GGUF, CUDA) — TinyLlama 1.1B"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    
    START=$(date +%s%N)
    OUTPUT=$($CANDLE_BIN --model "$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf" \
        --prompt "$PROMPT" \
        --sample-len 128 2>&1)
    END=$(date +%s%N)
    ELAPSED=$(( (END - START) / 1000000 ))
    TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
    
    echo ""
    echo "  ⏱  Total time: ${ELAPSED}ms"
    echo "  📊 Approx tok/s: $TOKS"
    echo "  📝 Output (first 200 chars):"
    echo "$OUTPUT" | tail -5 | head -3 | sed 's/^/     /'
else
    echo "  ✗ candle build failed"
    # Try without CUDA
    cargo build --release --example quantized 2>&1 | tail -5
    CANDLE_BIN="./target/release/examples/quantized"
    if [ -f "$CANDLE_BIN" ]; then
        echo "  ✓ candle quantized (CPU) built"
        START=$(date +%s%N)
        OUTPUT=$($CANDLE_BIN --model "$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf" \
            --prompt "$PROMPT" \
            --sample-len 128 2>&1)
        END=$(date +%s%N)
        ELAPSED=$(( (END - START) / 1000000 ))
        TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
        echo "  ⏱  Total time: ${ELAPSED}ms (CPU)"
        echo "  📊 Approx tok/s: $TOKS"
    fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  BENCHMARK COMPLETE                                          ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "Previous results (from first benchmark):"
echo "  KoboldCPP (CUDA):  94.7 tok/s"
echo "  wgpu-llm (Vulkan): 26.0 tok/s"
echo ""
echo "Check above for Crane CUDA, mistral.rs, and candle results."
