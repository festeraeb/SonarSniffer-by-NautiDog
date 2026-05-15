#!/bin/bash
# Agent dispatch helper — send prompts to local LLMs
# Usage: ./agent_dispatch.sh [moe|r1] "prompt" [max_tokens]

AGENT="${1:-moe}"
PROMPT="$2"
MAX_TOKENS="${3:-2048}"

case "$AGENT" in
  moe|coder|writer)
    PORT=5001
    NAME="Moe (Qwen 35B — Writer)"
    ;;
  r1|thinker|reviewer)
    PORT=5555
    NAME="R1 7B (Thinker)"
    ;;
  *)
    echo "Unknown agent: $AGENT (use moe/r1)"
    exit 1
    ;;
esac

echo ">>> Dispatching to $NAME on port $PORT..."
RESPONSE=$(curl -s http://127.0.0.1:$PORT/api/v1/generate \
  -X POST -H "Content-Type: application/json" \
  -d "{\"prompt\":\"$PROMPT\",\"max_length\":$MAX_TOKENS,\"temperature\":0.4}" \
  --max-time 120)

echo "$RESPONSE" | python3 -c "import sys,json; r=json.load(sys.stdin); print(r['results'][0]['text'])" 2>/dev/null || echo "$RESPONSE"

# Print speed
PERF=$(curl -s http://127.0.0.1:$PORT/api/extra/perf)
SPEED=$(echo "$PERF" | python3 -c "import sys,json; p=json.load(sys.stdin); print(f\"{p['last_eval_speed']:.1f} t/s\")" 2>/dev/null)
echo ""
echo "--- [$NAME] $SPEED ---"
