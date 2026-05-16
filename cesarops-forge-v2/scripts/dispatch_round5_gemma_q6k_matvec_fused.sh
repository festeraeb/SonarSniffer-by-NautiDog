#!/bin/bash
# Dispatch to Gemma-4-26B-MoE on P100 #0 (port 5001)
# Task: Fused Q6_K dequant + matvec WGSL kernel.
#
# This is the biggest theoretical perf win — eliminates the
# Q6_K weights -> dequant -> staging f32 buffer -> matvec roundtrip.
# Single shader does both, weights stay packed until the multiply.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

PROMPT='Output ONE complete WGSL shader file `matvec_q6k_fused.wgsl`. NO commentary, NO markdown fences, NO <think> blocks. Only the .wgsl file.

GOAL: fuse Q6_K dequantization with matrix-vector multiply in one kernel.
output[n] = sum_k (Q6Kdequant(W[n*K + k]) * input[k])

Q6_K BLOCK LAYOUT (210 bytes per 256 elements, GGUF native order):
  bytes [0..127]:   ql — 128 bytes, lower-4 nibbles, interleaved
  bytes [128..191]: qh — 64 bytes,  upper-2 bits, interleaved
  bytes [192..207]: scales — 16 signed-i8 sub-block scales
  bytes [208..209]: d — fp16 super-block scale

DEQUANT MATH (matches llama.cpp dequantize_row_q6_K):
  Each 256-element super-block has TWO halves of 128 elements.
  Inside each half, an inner loop l = 0..32 produces FOUR outputs at
  positions {l, l+32, l+64, l+96} from interleaved ql/qh nibbles.
  Sub-block scales for a half are at scales[half*8 + (0,2,4,6)].
  Per element value = d * scale * ((ql_nibble | (qh_bits << 4)) - 32)

BINDINGS + PARAMS:
  struct Params { N: u32, K: u32, K_blocks: u32, _pad: u32 }
  @group(0) @binding(0) var<storage, read>       input:    array<f32>;     // [K]
  @group(0) @binding(1) var<storage, read>       q6k_data: array<u32>;     // packed bytes
  @group(0) @binding(2) var<storage, read_write> output:   array<f32>;     // [N]
  var<push_constant> params: Params;
  @compute @workgroup_size(256, 1, 1)
  fn main(@builtin(global_invocation_id) gid: vec3<u32>) { ... }

ALGORITHM:
  let n = gid.x;
  if (n >= params.N) return;
  var acc: f32 = 0.0;
  // For each super-block b in row n (K_blocks total):
  //   block_byte_off = (n * K_blocks + b) * 210
  //   read d (fp16 -> f32) at byte 208
  //   for half in 0..2:
  //     for l in 0..32:
  //       compute 4 dequanted weights at positions l, l+32, l+64, l+96
  //       multiply each by input[(b*256) + half*128 + corresponding pos] and accumulate
  output[n] = acc;

HELPERS YOU MUST INCLUDE:
  fn read_byte(byte_off: u32) -> u32 (read individual byte from q6k_data array<u32>)
  fn signed_byte(b: u32) -> i32 (sign-extend i8)
  fn fp16_to_f32(bits: u32) -> f32

Match the exact dequant slot logic from the existing shaders/dequant_q6k.wgsl
(slot 0..3 maps to qh shifts 0,2,4,6 and ql_a vs ql_b alternation).

Return ONLY the .wgsl file content.'

ESC=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 1500 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC, \"max_length\": 4096, \"temperature\": 0.15, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/r5_gemma_q6k_fused.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r5_gemma_q6k_fused.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r5_gemma_q6k_fused.wgsl" 2>"$OUT_DIR/r5_gemma_q6k_fused.err"
echo "[Gemma-4 r5] q6k_matvec_fused: $(wc -l < "$OUT_DIR/r5_gemma_q6k_fused.wgsl") lines"
