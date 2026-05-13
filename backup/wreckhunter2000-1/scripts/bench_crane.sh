#!/bin/bash
# Benchmark Crane (crane-oai) as an API server
set -e
source "$HOME/.cargo/env" 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
CRANE_BIN="$BENCH_DIR/Crane/target/release/crane-oai"
MODEL_DIR="$BENCH_DIR/models/qwen25-1.5b"
PORT=5556
PROMPT="The Great Lakes region of North America contains numerous shipwrecks from the 19th and 20th centuries. The most effective method for detecting submerged wrecks using satellite imagery involves"

echo "Starting Crane server on port $PORT..."
$CRANE_BIN --model-path "$MODEL_DIR" --port $PORT --format safetensors --model-type qwen25 2>&1 &
CRANE_PID=$!
echo "Crane PID: $CRANE_PID"

# Wait for server ready
echo "Waiting for Crane to load model..."
for i in $(seq 1 90); do
    if curl -s http://localhost:$PORT/v1/models > /dev/null 2>&1; then
        echo "  Server ready after ${i}s"
        break
    fi
    if ! kill -0 $CRANE_PID 2>/dev/null; then
        echo "  ERROR: Crane process died"
        wait $CRANE_PID 2>/dev/null
        EXIT_CODE=$?
        echo "  Exit code: $EXIT_CODE"
        exit 1
    fi
    sleep 1
done

if ! curl -s http://localhost:$PORT/v1/models > /dev/null 2>&1; then
    echo "  ERROR: Crane never became ready (90s timeout)"
    kill $CRANE_PID 2>/dev/null
    exit 1
fi

echo ""
echo "Running inference benchmark..."
START=$(date +%s%N)

RESP=$(curl -s -X POST http://localhost:$PORT/v1/completions \
    -H "Content-Type: application/json" \
    -d "{\"model\":\"tinyllama\",\"prompt\":\"$PROMPT\",\"max_tokens\":128,\"temperature\":0.7}")

END=$(date +%s%N)
ELAPSED=$(( (END - START) / 1000000 ))

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Crane Results (TinyLlama 1.1B, GTX 1070)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Total time: ${ELAPSED}ms"
TOKS=$(python3 -c "print(f'{128 / ($ELAPSED / 1000.0):.2f}')")
echo "  Approx tok/s: $TOKS"
echo ""
echo "  Response:"
echo "$RESP" | python3 -m json.tool 2>/dev/null | head -25
echo ""

# Cleanup
kill $CRANE_PID 2>/dev/null
wait $CRANE_PID 2>/dev/null || true
echo "Crane stopped."
