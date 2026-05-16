#!/bin/bash
# Re-dispatch the push-constants task to Gemma-4-MoE (P100 #0, port 5001)
# Qwen3.6-A3B failed it (chain-of-thought mode). Gemma-4 produced clean code
# in round 2, so it gets the retry.
#
# Asks for FULL FILE OUTPUT, not a diff (per the lesson learned about diffs).
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='You are a Rust + wgpu expert. Migrate the matvec compute pipeline from uniform-buffer params to push constants for lower per-dispatch overhead on Pascal GPUs.

Output THREE files in full (not diffs). Each file separated by a marker line `===== FILE: <path> =====` on its own line, then the complete file content, no markdown fences.

FILE 1: shaders/matvec.wgsl
  - struct Params { N: u32, K: u32, _pad0: u32, _pad1: u32 }
  - bindings: input @group(0)@binding(0), weights @group(0)@binding(1), output @group(0)@binding(2)
  - replace `@group(0) @binding(3) var<uniform> params: Params;` with `var<push_constant> params: Params;`
  - keep workgroup_size(256), one row per thread, 8x unroll
  - shader main reads params.N and params.K like before

FILE 2: src/pipeline_init.rs (matvec section only — output the WHOLE file with the matvec changes)
  - drop binding 3 from the matvec_bgl bind_group_layout
  - in the pipeline_layout descriptor for matvec, set push_constant_ranges to a slice with one wgpu::PushConstantRange { stages: wgpu::ShaderStages::COMPUTE, range: 0..16 }
  - keep all other pipelines (rmsnorm, dequant_q6k, swiglu, attention, softmax, attn_value, rope, matvec_bias, dequant_q4km, dequant_iq4xs) unchanged

FILE 3: src/forward_pass.rs (only the dispatch_matvec helper — output the WHOLE file with that function changed)
  - in dispatch_matvec: drop the params_buf creation and bind_group entry for params
  - after `pass.set_pipeline(&pipelines.matvec)` and before `pass.dispatch_workgroups(...)`, call `pass.set_push_constants(0, bytemuck::cast_slice(&[n as u32, k as u32, 0u32, 0u32]))`
  - keep dispatch_matvec_bias UNCHANGED (still uses uniform buffer)
  - all other functions unchanged

Reply with: `===== FILE: shaders/matvec.wgsl =====` then the file, `===== FILE: src/pipeline_init.rs =====` then that file, `===== FILE: src/forward_pass.rs =====` then that file. No commentary, no thinking blocks, no markdown fences anywhere.'

ESC=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 1200 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC, \"max_length\": 8192, \"temperature\": 0.15, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/r3_gemma_pushconst.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r3_gemma_pushconst.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r3_gemma_pushconst.txt" 2>"$OUT_DIR/r3_gemma_pushconst.err"
echo "[Gemma-4 round 3] pushconst.txt: $(wc -l < "$OUT_DIR/r3_gemma_pushconst.txt") lines"
