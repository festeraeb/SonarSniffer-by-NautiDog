#!/bin/bash
# deploy_and_test.sh — rebuild and test cesarops-inference on T440
# Run with: bash /tmp/deploy_and_test.sh

set -e

PROJ="/codebase/repos/wreckhunter2000-1/cesarops-inference"
MODEL="/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"
PORT=5002

source /home/cesarops/.cargo/env

# Kill any existing instance
pkill -f "cesarops-inference.*${PORT}" 2>/dev/null || true
sleep 1

# Build
cd "$PROJ"
touch src/*.rs
echo "=== BUILDING ==="
cargo build --release 2>&1 | grep -E "Compiling|Finished|error"

# Start server
echo "=== STARTING SERVER ==="
nohup ./target/release/cesarops-inference --model "$MODEL" --port "$PORT" > /tmp/inference.log 2>&1 &
SERVER_PID=$!
echo "Server PID: $SERVER_PID"
sleep 4

# Check if it's still alive
if ! kill -0 $SERVER_PID 2>/dev/null; then
    echo "=== SERVER CRASHED ==="
    cat /tmp/inference.log
    exit 1
fi

# Test
echo "=== TESTING ==="
RESULT=$(curl -s -X POST http://127.0.0.1:${PORT}/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Hello","max_length":3,"temperature":0.7}')
echo "Response: $RESULT"

# Show logs
echo ""
echo "=== LOGS ==="
tail -30 /tmp/inference.log
