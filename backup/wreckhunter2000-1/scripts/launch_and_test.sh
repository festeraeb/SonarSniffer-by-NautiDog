#!/bin/bash
# Kill any existing instance
pkill -f "cesarops-inference.*5002" 2>/dev/null
sleep 1

# Start the server
cd /codebase/repos/wreckhunter2000-1/cesarops-inference
nohup ./target/release/cesarops-inference --model /codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf --port 5002 > /tmp/inference.log 2>&1 &
echo "Server PID: $!"

# Wait for startup
sleep 3

# Show startup logs
echo "=== STARTUP LOGS ==="
cat /tmp/inference.log

# Test inference
echo ""
echo "=== INFERENCE TEST ==="
curl -s -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Hello","max_length":3,"temperature":0.7}'
echo ""

# Show any new logs after the request
sleep 5
echo ""
echo "=== POST-REQUEST LOGS ==="
cat /tmp/inference.log | tail -20
