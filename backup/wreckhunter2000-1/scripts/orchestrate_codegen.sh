#!/usr/bin/env bash
# Cross-node code generation workflow orchestrator
#
# Usage:
#   # Single-node (auto-detect):
#   bash scripts/orchestrate_codegen.sh "Refactor tile_store to support LanceDB integration"
#
#   # Distributed (i7 reasoning → T440 execution):
#   bash scripts/orchestrate_codegen.sh \
#     --reasoning http://10.0.0.56:8765 \
#     --execution http://10.0.0.61:8766 \
#     --task "..."
#
# This script demonstrates the split-agent workflow:
#   1. ReasoningLead (i7 8GB+) runs --mode reason
#      → outputs TechnicalBrief JSON (architecture, design decisions, filter params)
#   2. ExecutionWorker (T440 6GB) daemon accepts the brief
#      → runs --mode code, generates Rust/WGSL implementation
#   3. Results are saved to output.json

set -euo pipefail

# ── Defaults ──────────────────────────────────────────────────────────────────
REASONING_NODE="${REASONING_BASE_URL:-https://llm.cesarops.org/v1}"
EXECUTION_NODE="${EXECUTION_BASE_URL:-http://10.0.0.61:8766}"
SPLIT_AGENT_BIN="${SPLIT_AGENT_BIN:-./target/release/model-team-tool}"
OUTPUT_FILE="output_codegen.json"

# Parse arguments
TASK=""
while [[ $# -gt 0 ]]; do
    case "$1" in
        --reasoning)
            REASONING_NODE="$2"
            shift 2
            ;;
        --execution)
            EXECUTION_NODE="$2"
            shift 2
            ;;
        --task)
            TASK="$2"
            shift 2
            ;;
        --output)
            OUTPUT_FILE="$2"
            shift 2
            ;;
        *)
            # Positional arg = task
            TASK="$1"
            shift
            ;;
    esac
done

if [[ -z "$TASK" ]]; then
    echo "Usage: $0 [--reasoning URL] [--execution URL] [--output FILE] <task>"
    echo ""
    echo "Example:"
    echo "  $0 --reasoning http://10.0.0.56:5001/v1 --execution http://10.0.0.61:8766 \\"
    echo "     'Optimize nauticuvs tile slicer for 16-square synthetic grids'"
    exit 1
fi

echo "═══════════════════════════════════════════════════════"
echo "  Split-Agent Codegen Orchestrator"
echo "═══════════════════════════════════════════════════════"
echo ""
echo "Task:     $TASK"
echo "Reasoning: $REASONING_NODE"
echo "Execution: $EXECUTION_NODE"
echo "Output:   $OUTPUT_FILE"
echo ""

# ── Step 1: Check connectivity ────────────────────────────────────────────────
echo "[1] Checking node connectivity..."

if ! curl -sf "${EXECUTION_NODE}/health" > /dev/null 2>&1; then
    echo "    ✗ ExecutionWorker not reachable at $EXECUTION_NODE"
    echo "    Check: curl ${EXECUTION_NODE}/health"
    exit 1
fi
echo "    ✓ ExecutionWorker ready at $EXECUTION_NODE"

# ── Step 2: Run reasoning phase locally ────────────────────────────────────────
# If reasoning URL is localhost:5001, we assume running on the ReasoningLead node
# Otherwise, you'd need to dispatch this phase to the remote node via SSH

echo ""
echo "[2] Running Reasoning phase (planning)..."
echo "    Endpoint: $REASONING_NODE"

if ! command -v "$SPLIT_AGENT_BIN" > /dev/null 2>&1 && [[ ! -f "$SPLIT_AGENT_BIN" ]]; then
    echo "    ✗ split-agent binary not found at $SPLIT_AGENT_BIN"
    echo "    Build it: cargo build --release -p model-team-tool"
    exit 1
fi

# Run reasoning phase (generates TechnicalBrief)
BRIEF_FILE="/tmp/technical_brief_$$.json"

"$SPLIT_AGENT_BIN" \
    --mode reason \
    --task "$TASK" \
    --reasoning-url "$REASONING_NODE" \
    --reasoning-temperature 0.2 \
    --reasoning-max-tokens 600 \
    --output "$BRIEF_FILE" \
    2>&1 | grep -E "^\[|Brief|Error" || true

if [[ ! -f "$BRIEF_FILE" ]]; then
    echo "    ✗ Failed to generate TechnicalBrief"
    exit 1
fi
echo "    ✓ TechnicalBrief generated"
echo "    File: $BRIEF_FILE"

# Pretty-print brief
echo "    Brief summary:"
grep -E '"architecture"|"approach"|"key_decisions"' "$BRIEF_FILE" | head -3 | sed 's/^/      /'

# ── Step 3: Send brief to ExecutionWorker ─────────────────────────────────────
echo ""
echo "[3] Dispatching to ExecutionWorker..."
echo "    Sending brief to POST $EXECUTION_NODE/brief"

RESPONSE_FILE="/tmp/code_response_$$.json"

if ! curl -s -X POST \
    -H "Content-Type: application/json" \
    -d @"$BRIEF_FILE" \
    "${EXECUTION_NODE}/brief" \
    > "$RESPONSE_FILE" 2>&1; then
    echo "    ✗ Request failed"
    cat "$RESPONSE_FILE" | head -10
    rm -f "$BRIEF_FILE" "$RESPONSE_FILE"
    exit 1
fi

# Check for HTTP errors
if grep -q '"error"' "$RESPONSE_FILE"; then
    echo "    ✗ ExecutionWorker returned error:"
    grep '"error"' "$RESPONSE_FILE"
    rm -f "$BRIEF_FILE" "$RESPONSE_FILE"
    exit 1
fi

echo "    ✓ ExecutionWorker processed brief"

# ── Step 4: Extract and save results ──────────────────────────────────────────
echo ""
echo "[4] Extracting results..."

# Extract generated code from response
if jq -e '.code' "$RESPONSE_FILE" > /dev/null 2>&1; then
    jq '.code' -r "$RESPONSE_FILE" > "${OUTPUT_FILE%.json}.rs"
    CODE_SIZE=$(wc -l < "${OUTPUT_FILE%.json}.rs")
    echo "    ✓ Generated Rust code: ${OUTPUT_FILE%.json}.rs ($CODE_SIZE lines)"
else
    echo "    ✗ No code in response"
    cat "$RESPONSE_FILE" | head -20
    rm -f "$BRIEF_FILE" "$RESPONSE_FILE"
    exit 1
fi

# Save full response
cp "$RESPONSE_FILE" "$OUTPUT_FILE"
echo "    ✓ Full response: $OUTPUT_FILE"

# ── Cleanup ───────────────────────────────────────────────────────────────────
rm -f "$BRIEF_FILE" "$RESPONSE_FILE"

echo ""
echo "═══════════════════════════════════════════════════════"
echo "  ✓ Codegen complete"
echo "  Generated code: ${OUTPUT_FILE%.json}.rs"
echo "═══════════════════════════════════════════════════════"
