#!/bin/bash
# Dispatch to Gemma-4-26B-MoE on P100 #0 (port 5001)
# Task: vec4 + push-constant fast path for fused matvec+bias kernel.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r4_gemma_matvec_bias_pc.prompt" << 'PROMPT_EOF'
You are a WGSL shader expert. Produce ONE complete WGSL shader file matvec_bias_vec4_pc.wgsl that fuses matrix-vector multiply, vec4 loads, push-constant params, AND bias add.

REFERENCE: an existing already-shipped shader in this same engine, matvec_vec4_pc.wgsl, uses these declarations:

struct Params { N: u32, K: u32, K_vec4: u32, _pad: u32 }
@group(0) @binding(0) var<storage, read>       input:   array<vec4<f32>>;
@group(0) @binding(1) var<storage, read>       weights: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output:  array<f32>;
var<push_constant> params: Params;

The existing matvec_bias.wgsl computes: output[n] = sum_k(W[n*K + k] * input[k]) + bias[n] where bias is array<f32> length N.

YOUR TASK — produce matvec_bias_vec4_pc.wgsl with these declarations:

struct Params { N: u32, K: u32, K_vec4: u32, _pad: u32 }
@group(0) @binding(0) var<storage, read>       input:   array<vec4<f32>>;
@group(0) @binding(1) var<storage, read>       weights: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read_write> output:  array<f32>;
@group(0) @binding(3) var<storage, read>       bias:    array<f32>;
var<push_constant> params: Params;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // Each thread computes one output row n; early-return if n >= params.N.
    // vec4<f32> accumulator. 4-iteration outer unroll over K_vec4.
    // After the loop, output[n] = acc.x + acc.y + acc.z + acc.w + bias[n]
}

Constraint: K must be a multiple of 4 (always true for transformer hidden dims).

Return ONLY the .wgsl file. NO markdown fences. NO commentary. NO <think> blocks.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r4_gemma_matvec_bias_pc.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 1500, 'temperature': 0.15, 'top_p': 0.9, 'rep_pen': 1.05}))")

curl -s -m 600 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r4_gemma_matvec_bias_pc.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r4_gemma_matvec_bias_pc.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r4_gemma_matvec_bias_pc.wgsl" 2>"$OUT_DIR/r4_gemma_matvec_bias_pc.err"
echo "[Gemma-4 r4] matvec_bias_vec4_pc.wgsl: $(wc -l < "$OUT_DIR/r4_gemma_matvec_bias_pc.wgsl") lines"
