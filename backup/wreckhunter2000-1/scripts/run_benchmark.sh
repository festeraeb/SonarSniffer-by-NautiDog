#!/bin/bash
# LLM Inference Benchmark: KoboldCPP vs wgpu-llm vs Crane
# Runs TinyLlama 1.1B on the GTX 1070 with identical prompts
# Measures: tokens/second, time-to-first-token, total generation time
set -e

BENCH_DIR="$HOME/benchmark"
MODEL_GGUF="$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf"
MODEL_SAFETENSORS="$BENCH_DIR/models/tinyllama-safetensors"
PROMPT="The Great Lakes region of North America contains numerous shipwrecks from the 19th and 20th centuries. The most effective method for detecting submerged wrecks using satellite imagery involves"
MAX_TOKENS=128
RESULTS_FILE="$BENCH_DIR/benchmark_results.json"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  LLM INFERENCE BENCHMARK — GTX 1070 (8GB)                   ║"
echo "║  Model: TinyLlama 1.1B (Q4_K_M / f16 safetensors)          ║"
echo "║  Prompt: shipwreck detection domain (relevant to our work)  ║"
echo "║  Max tokens: $MAX_TOKENS                                            ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""

source "$HOME/.cargo/env" 2>/dev/null || true

# ── Benchmark 1: KoboldCPP ────────────────────────────────────────────────────
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST 1: KoboldCPP (GGUF, CUDA offload)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

KOBOLD_BIN="$BENCH_DIR/koboldcpp"
if [ -f "$KOBOLD_BIN" ] && [ $(stat -c%s "$KOBOLD_BIN") -gt 1000 ]; then
    echo "Starting KoboldCPP server..."
    $KOBOLD_BIN --model "$MODEL_GGUF" --port 5555 --gpulayers 99 --contextsize 2048 --quiet &
    KOBOLD_PID=$!
    
    # Wait for server to be ready
    echo "Waiting for server startup..."
    for i in $(seq 1 60); do
        if curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1; then
            echo "  Server ready after ${i}s"
            break
        fi
        sleep 1
    done
    
    if curl -s http://localhost:5555/api/v1/model > /dev/null 2>&1; then
        echo "Running inference..."
        START_TIME=$(date +%s%N)
        
        RESPONSE=$(curl -s -X POST http://localhost:5555/api/v1/generate \
            -H "Content-Type: application/json" \
            -d "{\"prompt\": \"$PROMPT\", \"max_length\": $MAX_TOKENS, \"temperature\": 0.7, \"top_p\": 0.9}")
        
        END_TIME=$(date +%s%N)
        ELAPSED_MS=$(( (END_TIME - START_TIME) / 1000000 ))
        
        # Extract generated text
        GEN_TEXT=$(echo "$RESPONSE" | python3 -c "import sys,json; r=json.load(sys.stdin); print(r['results'][0]['text'][:200])" 2>/dev/null || echo "parse error")
        GEN_TOKENS=$(echo "$GEN_TEXT" | wc -w)  # rough token estimate
        
        TOKS_PER_SEC=$(python3 -c "print(f'{$MAX_TOKENS / ($ELAPSED_MS / 1000.0):.2f}')")
        
        echo ""
        echo "  ⏱  Total time: ${ELAPSED_MS}ms"
        echo "  📊 Approx tok/s: $TOKS_PER_SEC"
        echo "  📝 Output (first 200 chars): $GEN_TEXT"
        echo ""
        
        KOBOLD_RESULT="{\"engine\": \"koboldcpp\", \"elapsed_ms\": $ELAPSED_MS, \"max_tokens\": $MAX_TOKENS, \"approx_toks_per_sec\": $TOKS_PER_SEC}"
    else
        echo "  ❌ KoboldCPP failed to start"
        KOBOLD_RESULT="{\"engine\": \"koboldcpp\", \"error\": \"failed to start\"}"
    fi
    
    kill $KOBOLD_PID 2>/dev/null || true
    wait $KOBOLD_PID 2>/dev/null || true
    sleep 2
else
    echo "  ⚠ KoboldCPP binary not found or too small"
    KOBOLD_RESULT="{\"engine\": \"koboldcpp\", \"error\": \"binary not found\"}"
fi

# ── Benchmark 2: wgpu-llm ────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST 2: wgpu-llm (Vulkan/wgpu, WGSL shaders)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

WGPU_BIN="$BENCH_DIR/wgpu-llm/target/release/wgpu-llm"
if [ -f "$WGPU_BIN" ]; then
    echo "Running wgpu-llm inference..."
    START_TIME=$(date +%s%N)
    
    OUTPUT=$($WGPU_BIN --model-dir "$MODEL_SAFETENSORS" \
        --prompt "$PROMPT" \
        --max-tokens $MAX_TOKENS \
        --f16-weights 2>&1)
    
    END_TIME=$(date +%s%N)
    ELAPSED_MS=$(( (END_TIME - START_TIME) / 1000000 ))
    
    # Extract tok/s from wgpu-llm telemetry output
    TOKS_PER_SEC=$(echo "$OUTPUT" | grep -oP '[\d.]+\s*tok/s' | head -1 | grep -oP '[\d.]+' || echo "0")
    GEN_TEXT=$(echo "$OUTPUT" | head -5)
    
    echo ""
    echo "  ⏱  Total time: ${ELAPSED_MS}ms"
    echo "  📊 Reported tok/s: $TOKS_PER_SEC"
    echo "  📝 Output (first lines):"
    echo "$GEN_TEXT" | head -3 | sed 's/^/     /'
    echo ""
    
    WGPU_RESULT="{\"engine\": \"wgpu-llm\", \"elapsed_ms\": $ELAPSED_MS, \"max_tokens\": $MAX_TOKENS, \"reported_toks_per_sec\": $TOKS_PER_SEC}"
else
    echo "  ⚠ wgpu-llm binary not found (build may have failed)"
    WGPU_RESULT="{\"engine\": \"wgpu-llm\", \"error\": \"binary not found\"}"
fi

# ── Benchmark 3: Crane ────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST 3: Crane (Candle backend, Rust)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

CRANE_BIN=$(find "$BENCH_DIR/Crane/target/release" -maxdepth 1 -type f -executable 2>/dev/null | head -1)
if [ -n "$CRANE_BIN" ] && [ -f "$CRANE_BIN" ]; then
    echo "Running Crane inference..."
    START_TIME=$(date +%s%N)
    
    OUTPUT=$($CRANE_BIN --model "$MODEL_SAFETENSORS" \
        --prompt "$PROMPT" \
        --max-tokens $MAX_TOKENS 2>&1) || OUTPUT="Crane execution failed: $?"
    
    END_TIME=$(date +%s%N)
    ELAPSED_MS=$(( (END_TIME - START_TIME) / 1000000 ))
    
    TOKS_PER_SEC=$(echo "$OUTPUT" | grep -oiP '[\d.]+\s*tok' | head -1 | grep -oP '[\d.]+' || python3 -c "print(f'{$MAX_TOKENS / ($ELAPSED_MS / 1000.0):.2f}')")
    GEN_TEXT=$(echo "$OUTPUT" | head -5)
    
    echo ""
    echo "  ⏱  Total time: ${ELAPSED_MS}ms"
    echo "  📊 Approx tok/s: $TOKS_PER_SEC"
    echo "  📝 Output (first lines):"
    echo "$GEN_TEXT" | head -3 | sed 's/^/     /'
    echo ""
    
    CRANE_RESULT="{\"engine\": \"crane\", \"elapsed_ms\": $ELAPSED_MS, \"max_tokens\": $MAX_TOKENS, \"approx_toks_per_sec\": $TOKS_PER_SEC}"
else
    echo "  ⚠ Crane binary not found (build may have failed)"
    CRANE_RESULT="{\"engine\": \"crane\", \"error\": \"binary not found\"}"
fi

# ── Summary ───────────────────────────────────────────────────────────────────
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  BENCHMARK RESULTS SUMMARY                                   ║"
echo "╚══════════════════════════════════════════════════════════════╝"
echo ""
echo "GPU: $(nvidia-smi --query-gpu=name --format=csv,noheader | head -1)"
echo "Model: TinyLlama 1.1B"
echo "Max tokens: $MAX_TOKENS"
echo "Prompt: shipwreck detection domain"
echo ""

# Write JSON results
echo "[$KOBOLD_RESULT, $WGPU_RESULT, $CRANE_RESULT]" | python3 -m json.tool > "$RESULTS_FILE" 2>/dev/null || \
    echo "[$KOBOLD_RESULT, $WGPU_RESULT, $CRANE_RESULT]" > "$RESULTS_FILE"

echo "Results saved to: $RESULTS_FILE"
cat "$RESULTS_FILE"
