#!/bin/bash
# Dispatch to Qwen3.6-35B-A3B-MoE on P100 #1 (port 5002)
# Task: push-constant fast-path for attention QK^T shader.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

# Use a heredoc to a file to avoid bash quoting hell with shader code.
cat > "$OUT_DIR/r4_qwen_attention_pc.prompt" << 'PROMPT_EOF'
Output ONE complete WGSL shader file named attention_pc.wgsl. NO commentary. NO markdown fences. NO <think> blocks. ONLY the .wgsl file content.

PATTERN — match the existing attention.wgsl exactly except move params from a uniform buffer to a push constant:

struct Params {
    kv_len: u32,
    head_dim: u32,
    cur_pos: u32,
    scale: f32,
    kv_stride: u32,
    kv_head_offset: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<storage, read>       query:     array<f32>;
@group(0) @binding(1) var<storage, read>       key_cache: array<f32>;
@group(0) @binding(2) var<storage, read_write> scores:    array<f32>;
var<push_constant> params: Params;

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let i = gid.x;
    if (i >= params.kv_len) { return; }
    if (i > params.cur_pos) { scores[i] = -3.40282347e38; return; }

    let k_base = i * params.kv_stride + params.kv_head_offset;
    var dot: f32 = 0.0;
    let hd4 = params.head_dim & ~3u;
    var j: u32 = 0u;
    while (j < hd4) {
        dot = dot + query[j]      * key_cache[k_base + j];
        dot = dot + query[j + 1u] * key_cache[k_base + j + 1u];
        dot = dot + query[j + 2u] * key_cache[k_base + j + 2u];
        dot = dot + query[j + 3u] * key_cache[k_base + j + 3u];
        j = j + 4u;
    }
    while (j < params.head_dim) {
        dot = dot + query[j] * key_cache[k_base + j];
        j = j + 1u;
    }

    scores[i] = dot * params.scale;
}

Verify the WGSL is valid. Return ONLY the file. No fences. No commentary.
PROMPT_EOF

# Build JSON payload using python (handles all escaping)
PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r4_qwen_attention_pc.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 1500, 'temperature': 0.1, 'top_p': 0.85, 'rep_pen': 1.1}))")

curl -s -m 600 -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r4_qwen_attention_pc.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r4_qwen_attention_pc.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r4_qwen_attention_pc.wgsl" 2>"$OUT_DIR/r4_qwen_attention_pc.err"
echo "[Qwen3.6 r4] attention_pc.wgsl: $(wc -l < "$OUT_DIR/r4_qwen_attention_pc.wgsl") lines"
