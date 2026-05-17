#!/usr/bin/env bash
# Fire the same prompt at BOTH P100 endpoints in parallel.
# Returns whichever lands first / both for comparison.
#
# Usage:
#   ./dispatch_parallel.sh <prompt_file> <task_id>
#
# Outputs:
#   fleet_prompts/<task_id>_gemma.md   (P100#0 5001 Gemma-4-MoE)
#   fleet_prompts/<task_id>_qwen.md    (P100#1 5002 Qwen3.6-35B-A3B)
#
# Both run in background. Script returns immediately with PIDs.

set -e
PROMPT="$1"
TASK="$2"
[ -z "$PROMPT" ] || [ -z "$TASK" ] && { echo "Usage: $0 <prompt_file> <task_id>"; exit 1; }

DISPATCH="$(dirname "$0")/dispatch.sh"
DIR="$(dirname "$0")"

bash "$DISPATCH" http://127.0.0.1:5001 "$PROMPT" "$DIR/${TASK}_gemma.md" >> "$DIR/${TASK}_gemma.log" 2>&1 &
GEMMA_PID=$!

bash "$DISPATCH" http://127.0.0.1:5002 "$PROMPT" "$DIR/${TASK}_qwen.md" >> "$DIR/${TASK}_qwen.log" 2>&1 &
QWEN_PID=$!

echo "Parallel dispatch started:"
echo "  Gemma (5001) PID=$GEMMA_PID  -> $DIR/${TASK}_gemma.md"
echo "  Qwen  (5002) PID=$QWEN_PID  -> $DIR/${TASK}_qwen.md"
echo "Both running. Use 'wait' on PIDs or check files for completion."
