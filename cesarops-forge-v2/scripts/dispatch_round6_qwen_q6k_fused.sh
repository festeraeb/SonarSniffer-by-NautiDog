#!/bin/bash
# Dispatch to Qwen3.6-MoE on P100 #1 (port 5002) via OpenAI-compat endpoint.
# Task #2: Fused Q6_K dequant + matvec WGSL kernel.
# This is the biggest theoretical perf win — eliminates the
# Q6_K weights -> dequant -> staging f32 buffer -> matvec roundtrip.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r6_qwen_q6k_fused.prompt" << 'PROMPT_EOF'
Output ONE complete WGSL shader file matvec_q6k_fused.wgsl. NO markdown fences. NO commentary. NO <think> blocks. ONLY the .wgsl file content.

GOAL: fuse Q6_K dequantization with matrix-vector multiply in one kernel.
Per output row n: output[n] = sum over k of (Q6Kdequant(W[n*K + k]) * input[k]).

Q6_K BLOCK LAYOUT (210 bytes per 256 elements, GGUF native order):
  bytes [0..127]:   ql — 128 bytes, lower-4 nibbles, interleaved
  bytes [128..191]: qh — 64 bytes,  upper-2 bits, interleaved
  bytes [192..207]: scales — 16 signed-i8 sub-block scales
  bytes [208..209]: d — fp16 super-block scale

DEQUANT MATH (matches llama.cpp dequantize_row_q6_K):
  Each 256-element super-block has TWO halves of 128 elements.
  Inside each half, an inner loop l = 0..32 produces FOUR outputs at
  positions l, l+32, l+64, l+96 from interleaved ql/qh nibbles.
  Per element value = d * scale[is + slot*2] * ((ql_nibble | (qh_bits << 4)) - 32).
  Slot 0 uses ql_a low + qh shift 0; slot 1 uses ql_b low + qh shift 2;
  slot 2 uses ql_a high + qh shift 4; slot 3 uses ql_b high + qh shift 6.
  is = l / 16 within the half. sub-block scales for half h start at byte 192 + h*8.

REFERENCE ALREADY IN-TREE: shaders/dequant_q6k.wgsl computes these values
correctly with helpers fp16_to_f32, read_byte, signed_byte. Match its slot
logic exactly. The novelty here is folding the matvec accumulate into the
same kernel so we never write the 256 dequanted f32s to memory.

BINDINGS + PARAMS:
  struct Params { N: u32, K: u32, K_blocks: u32, _pad: u32 }
  @group(0) @binding(0) var<storage, read>       input:    array<f32>;
  @group(0) @binding(1) var<storage, read>       q6k_data: array<u32>;
  @group(0) @binding(2) var<storage, read_write> output:   array<f32>;
  var<push_constant> params: Params;
  @compute @workgroup_size(256, 1, 1)
  fn main(@builtin(global_invocation_id) gid: vec3<u32>) { ... }

ALGORITHM:
  let n = gid.x;
  if (n >= params.N) { return; }
  var acc: f32 = 0.0;
  for (var b = 0u; b < params.K_blocks; b = b + 1u) {
      let blk = (n * params.K_blocks + b) * 210u;
      let d = read_d_f16_as_f32(blk + 208u);
      // For half = 0 then half = 1:
      //   for l = 0..32:
      //     compute 4 dequanted weights at positions l, l+32, l+64, l+96
      //     multiply each by input[(b*256) + half*128 + corresponding_pos] and accumulate
  }
  output[n] = acc;

REQUIRED HELPERS (include them all):
  fn read_byte(byte_off: u32) -> u32 { ... read individual byte from q6k_data array<u32> ... }
  fn signed_byte(b: u32) -> i32 { return i32(b) - select(0, 256, b >= 128u); }
  fn fp16_to_f32(bits: u32) -> f32 { ... standard half-to-single conversion ... }

The shader must compile under naga. Return ONLY the .wgsl file content.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r6_qwen_q6k_fused.prompt').read(); print(json.dumps({'model':'qwen3.6','messages':[{'role':'user','content':p}],'max_tokens':3500,'temperature':0.15,'top_p':0.9}))")

curl -s -m 1200 -X POST http://127.0.0.1:5002/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r6_qwen_q6k_fused.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r6_qwen_q6k_fused.json')); print(d.get('choices',[{}])[0].get('message',{}).get('content',''))" \
  > "$OUT_DIR/r6_qwen_q6k_fused.wgsl" 2>"$OUT_DIR/r6_qwen_q6k_fused.err"
echo "[Qwen3.6 r6] q6k_fused: $(wc -l < "$OUT_DIR/r6_qwen_q6k_fused.wgsl") lines"
