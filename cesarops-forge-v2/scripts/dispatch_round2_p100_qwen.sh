#!/bin/bash
# Dispatch to Qwen3.6-35B-A3B-MoE on P100 #1 (port 5002)
# Task: Push-constants migration for matvec (Pascal speedup)
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='You are a Rust + wgpu expert. The cesarops-inference engine currently writes a uniform buffer per dispatch for matvec layer parameters (N, K). On Pascal GPUs this is overhead. Migrate matvec to use push constants instead.

Produce a unified diff (unified ASCII patch with `--- a/...` `+++ b/...` markers) that:

1. Modifies `shaders/matvec.wgsl` to declare push constants with `enable chromium_disable_uniformity_analysis;` if needed and `var<push_constant> params: Params;` instead of `@group(0) @binding(3) var<uniform> params: Params;` — keeping struct Params { N: u32, K: u32, _pad0: u32, _pad1: u32 } the same.

2. Modifies `src/pipeline_init.rs` matvec pipeline creation:
   - bind_group_layout: drop binding 3 (uniform)
   - pipeline_layout: add `push_constant_ranges: &[wgpu::PushConstantRange { stages: wgpu::ShaderStages::COMPUTE, range: 0..16 }]` (16 bytes for 4 u32s)
   - require device feature `wgpu::Features::PUSH_CONSTANTS` (caller must request it; just add a comment if it is not currently requested)

3. Modifies `src/forward_pass.rs` matvec dispatch helper:
   - Drop the params uniform buffer + write_buffer
   - Replace with `cpass.set_push_constants(0, bytemuck::cast_slice(&[N as u32, K as u32, 0u32, 0u32]))` after `set_pipeline` and before `dispatch_workgroups`

The patch must be self-contained (no commentary). If you cannot produce a patch, output: `// patch generation failed` and stop.'

ESC_PROMPT=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 900 -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC_PROMPT, \"max_length\": 3072, \"temperature\": 0.2, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/r2_qwen_pushconst.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r2_qwen_pushconst.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r2_qwen_pushconst.patch" 2>"$OUT_DIR/r2_qwen_pushconst.err"
echo "[Qwen3.6] pushconst.patch: $(wc -l < "$OUT_DIR/r2_qwen_pushconst.patch") lines"
