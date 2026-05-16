#!/bin/bash
# Dispatch to FortyTwo Rust Coder 14B (P100 #2, port 5002)
# Task: Fused Q6_K dequant + matvec WGSL shader for Pascal P100
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='You are a WGSL shader expert targeting Tesla P100 (Pascal sm_60, no tensor cores). Write a complete, working WGSL compute shader file matvec_q6k_fused.wgsl that fuses Q6_K dequantization with matrix-vector multiply.

CONSTRAINTS:
- WGSL syntax. Use @group/@binding, @compute, @workgroup_size.
- Q6_K block: 256 elements per super-block, two 128-elem halves; for each half an inner loop l=0..32 produces 4 outputs at positions l, l+32, l+64, l+96. Interleaved sub-block scales at is, is+2, is+4, is+6. Block bytes: 128 ql + 64 qh + 16 scales + 2 (f16 d) = 210 bytes.
- Bindings:
  @group(0) @binding(0) var<storage,read> input: array<f32>;
  @group(0) @binding(1) var<storage,read> q6k_blocks: array<u32>;
  @group(0) @binding(2) var<storage,read_write> output: array<f32>;
  @group(0) @binding(3) var<uniform> params: Params;
- workgroup_size(256). One thread = one output row. Iterate K_blocks super-blocks.
- Read d as bit-reinterpret of u16 from byte offset, dequant 256 elements following llama.cpp two-halves pattern, multiply-accumulate against input[k..k+256].
- Loop unrolling on the inner-32 loop. No dynamic branching in hot loop.

Return ONLY the complete .wgsl file content, no markdown fences, no commentary.'

ESC_PROMPT=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 600 -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC_PROMPT, \"max_length\": 2048, \"temperature\": 0.2, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/fortytwo_q6k_fused.json" 2>&1

python3 -c "import json,sys; d=json.load(open('$OUT_DIR/fortytwo_q6k_fused.json')); print(d.get('results',[{}])[0].get('text',''))" > "$OUT_DIR/fortytwo_q6k_fused.wgsl" 2>"$OUT_DIR/fortytwo_q6k_fused.err"
echo "FortyTwo done: $(wc -l < "$OUT_DIR/fortytwo_q6k_fused.wgsl" 2>/dev/null) lines"
