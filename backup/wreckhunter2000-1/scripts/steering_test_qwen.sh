#!/bin/bash
# Steering Test: Qwen2.5-1.5B vs TinyLlama
# Swaps KoboldCPP to Qwen2.5-1.5B safetensors and reruns the grounding test
set -e

BENCH_DIR="$HOME/benchmark"
LLM_URL="http://localhost:5555"
QWEN_MODEL="$BENCH_DIR/models/qwen25-1.5b"

echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  STEERING TEST: Qwen2.5-1.5B (code-focused architecture)   ║"
echo "║  Question: Does 50% more params cross the grounding floor?  ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# Kill existing KoboldCPP
echo ""
echo "[1/4] Swapping model to Qwen2.5-1.5B..."
pkill -f koboldcpp 2>/dev/null || true
sleep 3

# KoboldCPP doesn't natively load safetensors — it needs GGUF
# Check if we have a Qwen GGUF, if not download one
QWEN_GGUF="$BENCH_DIR/models/qwen2.5-1.5b-instruct-q4_k_m.gguf"
if [ ! -f "$QWEN_GGUF" ]; then
    echo "  Downloading Qwen2.5-1.5B-Instruct GGUF..."
    curl -L -o "$QWEN_GGUF" \
        "https://huggingface.co/Qwen/Qwen2.5-1.5B-Instruct-GGUF/resolve/main/qwen2.5-1.5b-instruct-q4_k_m.gguf" \
        2>&1 | tail -3
    echo "  Downloaded: $(ls -lh $QWEN_GGUF | awk '{print $5}')"
fi

# Start KoboldCPP with Qwen
echo "  Starting KoboldCPP with Qwen2.5-1.5B..."
$BENCH_DIR/koboldcpp --model "$QWEN_GGUF" --port 5555 --gpulayers 99 --contextsize 4096 --quiet &
KOBOLD_PID=$!

echo "  Waiting for model load..."
for i in $(seq 1 90); do
    if curl -s "$LLM_URL/api/v1/model" > /dev/null 2>&1; then
        echo "  ✓ Qwen2.5-1.5B ready after ${i}s (PID: $KOBOLD_PID)"
        break
    fi
    sleep 1
done

if ! curl -s "$LLM_URL/api/v1/model" > /dev/null 2>&1; then
    echo "  ✗ Failed to start. Aborting."
    exit 1
fi

QUESTION="What is the specific threshold logic used for glint suppression in the CESARops thermal_specialist? Reference the exact function names, parameter values, and file paths from the codebase context provided."

# ══════════════════════════════════════════════════════════════════════════════
# TEST A: UNSTEERED
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[2/4] TEST A: UNSTEERED (Qwen2.5-1.5B, no injection)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

RESP_A=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "{\"prompt\": \"Question: $QUESTION\\nAnswer:\", \"max_length\": 250, \"temperature\": 0.7}")

TEXT_A=$(echo "$RESP_A" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(error: {e})')
")

echo "$TEXT_A" | fold -w 78 | sed 's/^/  /'

# ══════════════════════════════════════════════════════════════════════════════
# TEST B: STEERED (nautivecs injection)
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[3/4] TEST B: STEERED (Qwen2.5-1.5B + nautivecs context)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

STEERED_JSON=$(python3 << 'PYEOF'
import json

context = """### INJECTED CODEBASE CONTEXT (from nautivecs)

FILE: tpu_client.py (lines 89-102) — _cpu_fallback_inference()
```python
def _cpu_fallback_inference(self, image) -> dict:
    arr = np.array(img.convert('L'), dtype=np.float32)
    thresh = float(np.nanpercentile(arr, 99.5))
    bright_pct = float(np.mean(arr >= thresh))
    glint_score = min(bright_pct * 20.0, 1.0)   # scale 0.5% bright pixels -> 0.1 score
    gy = arr[1:, :] - arr[:-1, :]
    jitter_score = min(float(np.std(gy)) / 64.0, 1.0)
    return {
        'glint_score': round(glint_score, 4),
        'jitter_score': round(jitter_score, 4),
        'pass': glint_score < 0.5 and jitter_score < 0.5,
    }
```

FILE: cesarops-slicer/src/thermal_specialist.rs (lines 241-248)
```rust
println!("Dispatching WGPU Thermal Submersion compute pass over {} points...", _stac_thermal_data.len());
let mut encoder = primary_device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });
{
    let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: None, timestamp_writes: None });
    cpass.set_pipeline(&compute_pipeline);
    cpass.set_bind_group(0, Some(&bind_group), &[]);
}
```

FILE: cesarops-hybrid-engine/src/cluster.rs (lines 33-38)
```rust
/// Represents one Tesla P100 GPU with its wgpu device, queue, and pre-allocated buffers.
pub struct P100Node {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}
```

## Grounding Rules
- The code context above is from the ACTUAL codebase. Reference it directly.
- If you mention a function, struct, or parameter, it MUST exist in the context above.
- If unsure whether something exists, say so rather than inventing it.
- When suggesting parameter values, reference the ranges you see in the actual code.
"""

question = "What is the specific threshold logic used for glint suppression in the CESARops thermal_specialist? Reference the exact function names, parameter values, and file paths from the codebase context provided."

prompt = f"{context}\n\nQuestion: {question}\nAnswer:"
print(json.dumps({"prompt": prompt, "max_length": 250, "temperature": 0.7}))
PYEOF
)

RESP_B=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "$STEERED_JSON")

TEXT_B=$(echo "$RESP_B" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(error: {e})')
")

echo "$TEXT_B" | fold -w 78 | sed 's/^/  /'

# ══════════════════════════════════════════════════════════════════════════════
# TEST C: CORRECTION INJECTED
# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "[4/4] TEST C: CORRECTION INJECTED (human override)"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

CORRECTED_JSON=$(python3 << 'PYEOF'
import json

correction = """## [CRITICAL: PREVIOUS HUMAN CORRECTIONS]

- **tune_parameters** (2026-05-07): For calm water tiles, the glint threshold of 0.5 is TOO AGGRESSIVE. Use 0.3 instead. The 99.5th percentile catches too many sun reflections on calm days.
- **tune_parameters** (2026-05-06): The jitter_score divisor of 64.0 is correct for Great Lakes but should be 128.0 for ocean deployments.

"""

context = """### INJECTED CODEBASE CONTEXT (from nautivecs)

FILE: tpu_client.py (lines 89-102) — _cpu_fallback_inference()
```python
def _cpu_fallback_inference(self, image) -> dict:
    arr = np.array(img.convert('L'), dtype=np.float32)
    thresh = float(np.nanpercentile(arr, 99.5))
    bright_pct = float(np.mean(arr >= thresh))
    glint_score = min(bright_pct * 20.0, 1.0)
    gy = arr[1:, :] - arr[:-1, :]
    jitter_score = min(float(np.std(gy)) / 64.0, 1.0)
    return {
        'glint_score': round(glint_score, 4),
        'jitter_score': round(jitter_score, 4),
        'pass': glint_score < 0.5 and jitter_score < 0.5,
    }
```

## Grounding Rules
- Code context above is from the ACTUAL codebase. Reference it directly.
- Functions/parameters you mention MUST exist in the context above.
- HUMAN CORRECTIONS override your training data for that specific case.
"""

question = "What threshold should I use for glint detection on calm water tiles? The code says 0.5 but I recall we changed it."

prompt = f"{correction}{context}\n\nQuestion: {question}\nAnswer:"
print(json.dumps({"prompt": prompt, "max_length": 250, "temperature": 0.7}))
PYEOF
)

RESP_C=$(curl -s -X POST "$LLM_URL/api/v1/generate" \
    -H "Content-Type: application/json" \
    -d "$CORRECTED_JSON")

TEXT_C=$(echo "$RESP_C" | python3 -c "
import sys, json
try:
    r = json.load(sys.stdin)
    print(r['results'][0]['text'].strip())
except Exception as e: print(f'(error: {e})')
")

echo "$TEXT_C" | fold -w 78 | sed 's/^/  /'

# ══════════════════════════════════════════════════════════════════════════════
echo ""
echo "╔══════════════════════════════════════════════════════════════╗"
echo "║  COMPARISON: TinyLlama 1.1B vs Qwen2.5-1.5B                ║"
echo "╠══════════════════════════════════════════════════════════════╣"
echo "║  Key questions:                                              ║"
echo "║  1. Does Qwen stop inventing floaat()/thresh_scorer?         ║"
echo "║  2. Does it cite glint_score < 0.5 and bright_pct * 20.0?   ║"
echo "║  3. Does Test C respect the 0.3 correction override?        ║"
echo "║  4. Does it stay within the code shown, or invent beyond?   ║"
echo "╚══════════════════════════════════════════════════════════════╝"

# Cleanup — leave KoboldCPP running for further tests
echo ""
echo "KoboldCPP still running with Qwen2.5-1.5B (PID: $KOBOLD_PID)"
echo "Kill with: kill $KOBOLD_PID"
