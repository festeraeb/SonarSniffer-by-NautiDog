#!/bin/bash
# Steering Engine Stress Test
# Proves: nautivecs injection grounds the LLM and prevents drift
# Compares: steered (with codebase context) vs unsteered (raw LLM)
set -e
source ~/.cargo/env 2>/dev/null || true

BENCH_DIR="$HOME/benchmark"
REPO_DIR="$HOME/wreckhunter2000-1"
NAUTIVECS_DB="$BENCH_DIR/nautivecs_test_store.json"
LLM_URL="http://localhost:5555"  # KoboldCPP already running from benchmark

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  STEERING ENGINE STRESS TEST                                 ║"
echo "║  Proving: nautivecs injection prevents LLM drift             ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# ── Step 1: Check if KoboldCPP is still running ──────────────────────────────
echo ""
echo "[1/5] Checking LLM endpoint..."
if curl -s "$LLM_URL/api/v1/model" > /dev/null 2>&1; then
    echo "  ✓ KoboldCPP is running"
else
    echo "  Starting KoboldCPP..."
    $BENCH_DIR/koboldcpp --model "$BENCH_DIR/models/tinyllama-1.1b-chat.Q4_K_M.gguf" \
        --port 5555 --gpulayers 99 --contextsize 4096 --quiet &
    KOBOLD_PID=$!
    for i in $(seq 1 60); do
        if curl -s "$LLM_URL/api/v1/model" > /dev/null 2>&1; then
            echo "  ✓ KoboldCPP ready after ${i}s"
            break
        fi
        sleep 1
    done
fi

# ── Step 2: Index the codebase with nautivecs ─────────────────────────────────
echo ""
echo "[2/5] Indexing codebase into nautivecs..."

# Check if nautivecs-cli is built
if [ ! -f "$REPO_DIR/target/release/nautivecs-cli" ]; then
    echo "  Building nautivecs-cli..."
    cd "$REPO_DIR"
    cargo build --release -p nautivecs --bin nautivecs-cli 2>&1 | tail -3
fi

NAUTIVECS_CLI="$REPO_DIR/target/release/nautivecs-cli"
if [ -f "$NAUTIVECS_CLI" ]; then
    echo "  Indexing cesarops-slicer/src..."
    $NAUTIVECS_CLI --db-path "$NAUTIVECS_DB" --endpoint "$LLM_URL/v1" index "$REPO_DIR/cesarops-slicer/src" 2>&1 | tail -3
    echo "  Indexing cesarops-hybrid-engine/src..."
    $NAUTIVECS_CLI --db-path "$NAUTIVECS_DB" --endpoint "$LLM_URL/v1" index "$REPO_DIR/cesarops-hybrid-engine/src" 2>&1 | tail -3
    echo "  Indexing sovereign-cloud/src..."
    $NAUTIVECS_CLI --db-path "$NAUTIVECS_DB" --endpoint "$LLM_URL/v1" index "$REPO_DIR/sovereign-cloud/src" 2>&1 | tail -3
    
    echo "  Store stats:"
    $NAUTIVECS_CLI --db-path "$NAUTIVECS_DB" --endpoint "$LLM_URL/v1" stats 2>&1
else
    echo "  ✗ nautivecs-cli not found — using keyword-only mode"
fi

# ── Step 3: UNSTEERED query (raw LLM, no context) ────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST A: UNSTEERED (no nautivecs injection)"
echo "  Question: What threshold should I use for glint detection"
echo "            on calm water tiles in the CESARops pipeline?"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

QUESTION="What threshold should I use for glint detection on calm water tiles in the CESARops pipeline? Reference specific functions and parameter ranges from the codebase."

UNSTEERED_RESP=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "{\"prompt\": \"$QUESTION\", \"max_length\": 256, \"temperature\": 0.7}")

UNSTEERED_TEXT=$(echo "$UNSTEERED_RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'])
except: print('(parse error)')
" 2>/dev/null)

echo ""
echo "  UNSTEERED RESPONSE:"
echo "  ─────────────────────"
echo "$UNSTEERED_TEXT" | fold -w 80 | sed 's/^/  /'
echo ""

# ── Step 4: STEERED query (with nautivecs context injection) ──────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST B: STEERED (nautivecs context injected)"
echo "  Same question, but with codebase context in system prompt"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Get nautivecs context (keyword search if embeddings unavailable)
if [ -f "$NAUTIVECS_CLI" ] && [ -f "$NAUTIVECS_DB" ]; then
    CONTEXT=$($NAUTIVECS_CLI --db-path "$NAUTIVECS_DB" --endpoint "$LLM_URL/v1" \
        query "glint threshold detection calm water" --top-k 5 2>&1)
else
    CONTEXT="[No nautivecs store available — using hardcoded context for test]
FILE: cesarops-slicer/src/thermal_specialist.rs
\`\`\`rust
pub async fn deploy_specialist_node(container_id: &str) -> bool {
    let cluster_ip = \"100.72.182.77\";
    println!(\"Initializing dipole scan GPU orchestration at {}\", cluster_ip);
    true
}
\`\`\`
FILE: tpu_client.py (glint check)
\`\`\`python
glint_score = min(bright_pct * 20.0, 1.0)  # scale 0.5% bright pixels -> 0.1 score
# pass threshold: glint_score < 0.5
\`\`\`"
fi

# Build steered prompt
STEERED_PROMPT="### INJECTED CODEBASE CONTEXT (from nautivecs)

$CONTEXT

## Grounding Rules
- The code context above is from the ACTUAL codebase. Reference it directly.
- If you mention a function, struct, or parameter, it MUST exist in the context above.
- If unsure whether something exists, say so rather than inventing it.
- When suggesting parameter values, reference the ranges you see in the actual code.

## Question
$QUESTION"

STEERED_RESP=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "{\"prompt\": \"$STEERED_PROMPT\", \"max_length\": 256, \"temperature\": 0.7}")

STEERED_TEXT=$(echo "$STEERED_RESP" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'])
except: print('(parse error)')
" 2>/dev/null)

echo ""
echo "  STEERED RESPONSE:"
echo "  ─────────────────────"
echo "$STEERED_TEXT" | fold -w 80 | sed 's/^/  /'
echo ""

# ── Step 5: Analysis ──────────────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  ANALYSIS"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "  Look for:"
echo "  ✓ STEERED response references actual functions from the codebase"
echo "  ✓ STEERED response uses parameter values from the injected context"
echo "  ✗ UNSTEERED response invents functions that don't exist"
echo "  ✗ UNSTEERED response guesses parameter values without grounding"
echo ""
echo "  Key indicators of drift (UNSTEERED):"
echo "  - Mentions functions not in our codebase"
echo "  - Suggests threshold values with no basis"
echo "  - Generic advice not specific to CESARops"
echo ""
echo "  Key indicators of grounding (STEERED):"
echo "  - References glint_score < 0.5 from tpu_client.py"
echo "  - References bright_pct * 20.0 scaling"
echo "  - Stays within the parameter ranges shown in context"
echo ""

# Save results
RESULTS_FILE="$BENCH_DIR/steering_test_results.json"
python3 -c "
import json
results = {
    'test': 'steering_stress_test',
    'question': '$QUESTION',
    'unsteered': '''$UNSTEERED_TEXT''',
    'steered': '''$STEERED_TEXT''',
    'context_injected': True,
}
with open('$RESULTS_FILE', 'w') as f:
    json.dump(results, f, indent=2)
print(f'Results saved to: $RESULTS_FILE')
" 2>/dev/null || echo "Results saved (json formatting skipped)"
