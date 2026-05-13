#!/bin/bash
# Steering Stress Test v2 — works without repo on cesarops2
# Uses hardcoded context injection (simulating nautivecs output)
# Proves: injection grounds TinyLlama and prevents drift
set -e

LLM_URL="http://localhost:5555"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  STEERING ENGINE STRESS TEST v2                              ║"
echo "║  Model: TinyLlama 1.1B @ 95 tok/s                          ║"
echo "║  Test: Steered vs Unsteered vs Correction-Injected          ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# Check LLM
echo ""
echo "[CHECK] KoboldCPP..."
if ! curl -s "$LLM_URL/api/v1/model" > /dev/null 2>&1; then
    echo "  KoboldCPP not running. Start it first."
    exit 1
fi
echo "  ✓ KoboldCPP responding"

QUESTION="What is the specific threshold logic used for glint suppression in the CESARops thermal_specialist? Reference the exact function names, parameter values, and file paths."

# ══════════════════════════════════════════════════════════════════════════════
# TEST A: UNSTEERED (raw LLM, no context)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST A: UNSTEERED (no injection)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

RESP_A=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "{\"prompt\": \"Question: $QUESTION\\nAnswer:\", \"max_length\": 200, \"temperature\": 0.7}")

TEXT_A=$(echo "$RESP_A" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(parse error: {e})')
" 2>/dev/null)

echo ""
echo "$TEXT_A" | fold -w 78 | sed 's/^/  /'
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# TEST B: STEERED (nautivecs context injected — simulated)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST B: STEERED (nautivecs context injected)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# This simulates what nautivecs would return for "glint threshold thermal"
read -r -d '' INJECTED_CONTEXT << 'CONTEXT' || true
### INJECTED CODEBASE CONTEXT (from nautivecs)

FILE: tpu_client.py (lines 89-102) — check_glint_jitter()
```python
def _cpu_fallback_inference(self, image) -> dict:
    arr = np.array(img.convert('L'), dtype=np.float32)
    thresh = float(np.nanpercentile(arr, 99.5))
    bright_pct = float(np.mean(arr >= thresh))
    glint_score = min(bright_pct * 20.0, 1.0)   # scale 0.5% bright pixels -> 0.1 score
    # Jitter: measure local gradient variance as proxy
    gy = arr[1:, :] - arr[:-1, :]
    jitter_score = min(float(np.std(gy)) / 64.0, 1.0)
    return {
        'glint_score': round(glint_score, 4),
        'jitter_score': round(jitter_score, 4),
        'pass': glint_score < 0.5 and jitter_score < 0.5,
    }
```

FILE: cesarops-slicer/src/thermal_specialist.rs (lines 56-85)
```rust
/// Deploy an isolated worker container onto the cluster.
pub async fn deploy_specialist_node(container_id: &str) -> bool {
    let cluster_ip = "100.72.182.77";
    println!("Initializing dipole scan GPU orchestration at {}", cluster_ip);
    true
}

/// Dispatching WGPU Thermal Submersion compute pass
/// Uses wgpu compute shaders on P100 GPUs
/// Workgroup size: 256 threads
/// Input: STAC thermal band data (f32 array)
/// Output: anomaly scores per pixel
```

FILE: cesarops-hybrid-engine/src/cluster.rs (lines 33-55)
```rust
/// Represents one Tesla P100 GPU with its wgpu device, queue, and pre-allocated buffers.
pub struct P100Node {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    // Buffers are allocated once at startup and reused across role flips
}
```

## Grounding Rules
- The code context above is from the ACTUAL codebase. Reference it directly.
- If you mention a function, struct, or parameter, it MUST exist in the context above.
- If unsure whether something exists, say so rather than inventing it.
- When suggesting parameter values, reference the ranges you see in the actual code.
CONTEXT

STEERED_PROMPT="$INJECTED_CONTEXT

Question: $QUESTION
Answer:"

# Escape for JSON
STEERED_JSON=$(python3 -c "
import json, sys
prompt = '''$STEERED_PROMPT'''
print(json.dumps({'prompt': prompt, 'max_length': 200, 'temperature': 0.7}))
" 2>/dev/null)

RESP_B=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "$STEERED_JSON")

TEXT_B=$(echo "$RESP_B" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(parse error: {e})')
" 2>/dev/null)

echo ""
echo "$TEXT_B" | fold -w 78 | sed 's/^/  /'
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# TEST C: CORRECTION INJECTED (human override)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  TEST C: CORRECTION INJECTED (human feedback override)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

read -r -d '' CORRECTION_CONTEXT << 'CORRECTION' || true
## [CRITICAL: PREVIOUS HUMAN CORRECTIONS]

- **tune_parameters** (2026-05-07): For calm water tiles, the glint threshold of 0.5 is TOO AGGRESSIVE. Use 0.3 instead. The 99.5th percentile approach catches too many sun reflections on calm days. Reduce sensitivity.
- **tune_parameters** (2026-05-06): The jitter_score divisor of 64.0 is correct for Great Lakes but should be 128.0 for ocean deployments due to higher wave energy.

CORRECTION

CORRECTED_PROMPT="$CORRECTION_CONTEXT
$INJECTED_CONTEXT

Question: $QUESTION
Answer:"

CORRECTED_JSON=$(python3 -c "
import json
prompt = '''$CORRECTED_PROMPT'''
print(json.dumps({'prompt': prompt, 'max_length': 200, 'temperature': 0.7}))
" 2>/dev/null)

RESP_C=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "$CORRECTED_JSON")

TEXT_C=$(echo "$RESP_C" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(parse error: {e})')
" 2>/dev/null)

echo ""
echo "$TEXT_C" | fold -w 78 | sed 's/^/  /'
echo ""

# ══════════════════════════════════════════════════════════════════════════════
# SUMMARY
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  RESULTS SUMMARY                                             ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║                                                              ║"
echo "║  TEST A (Unsteered): Does it hallucinate functions?          ║"
echo "║  TEST B (Steered):   Does it reference actual code?          ║"
echo "║  TEST C (Corrected): Does it use the human override?         ║"
echo "║                                                              ║"
echo "║  Key things to verify:                                       ║"
echo "║  - B should mention: glint_score < 0.5, bright_pct * 20.0   ║"
echo "║  - C should mention: threshold 0.3 (not 0.5)                ║"
echo "║  - A should NOT reference real code (it has no context)      ║"
echo "║                                                              ║"
echo "╚══════════════════════════════════════════════════════════════╝"
