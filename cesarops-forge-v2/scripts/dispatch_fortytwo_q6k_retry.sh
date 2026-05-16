#!/bin/bash
# Retry to FortyTwo Rust Coder 14B with corrector notes
# Attempt 2/3 — first attempt used Rust syntax instead of WGSL
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"

PROMPT='Your previous attempt at matvec_q6k_fused.wgsl had errors. Fix them and produce a valid WGSL file:

ERRORS IN PREVIOUS ATTEMPT:
1. Used Rust syntax: `let mut acc = f32(0.0);` -- WGSL uses `var acc: f32 = 0.0;`
2. Used pointer syntax: `let ql_ptr = &q6k_blocks[...]` -- WGSL has no `&` references; index directly.
3. Used `bitcast<f16>(d_bytes)` -- f16 is not a primitive in default WGSL profile, and bitcast needs same-bit-width source. Read d as raw u32 byte slice and unpack manually OR store d pre-converted as f32.
4. Missing the `@group(0) @binding(N)` declarations entirely.
5. Q6_K dequant math wrong: should follow llama.cpp two-halves pattern where each output q = (ql_low | (qh_bits<<4)) - 32, then * d * scale_byte_signed.
6. Did not multiply by input[k] for the matvec accumulation.

CORRECT WGSL TEMPLATE TO COMPLETE:

```
struct Params { N: u32, K: u32, K_blocks: u32, _pad: u32 }

@group(0) @binding(0) var<storage, read>       input: array<f32>;
@group(0) @binding(1) var<storage, read>       q6k:   array<u32>;  // raw block bytes packed 4-per-u32
@group(0) @binding(2) var<storage, read_write> output: array<f32>;
@group(0) @binding(3) var<uniform>             params: Params;

// Helpers: read a single byte from u32 array at byte offset
fn read_u8(byte_off: u32) -> u32 {
  let w = q6k[byte_off >> 2u];
  let s = (byte_off & 3u) * 8u;
  return (w >> s) & 0xFFu;
}

fn read_i8(byte_off: u32) -> i32 {
  let b = read_u8(byte_off);
  // sign-extend from 8 bits
  return i32(b) - i32((b & 0x80u) << 1u);
}

// Read fp16 d at byte offset, return as f32
fn read_d_f16_as_f32(byte_off: u32) -> f32 {
  let lo = read_u8(byte_off);
  let hi = read_u8(byte_off + 1u);
  let h  = (hi << 8u) | lo;
  // IEEE754 binary16 -> binary32 conversion
  let sign = (h >> 15u) & 0x1u;
  let exp  = (h >> 10u) & 0x1Fu;
  let frac =  h         & 0x3FFu;
  if (exp == 0u) {
    if (frac == 0u) { return select(0.0, -0.0, sign == 1u); }
    // subnormal
    let f = f32(frac) * 0.0000000596046448; // 2^-24
    return select(f, -f, sign == 1u);
  }
  if (exp == 31u) {
    return select(3.4e38, -3.4e38, sign == 1u);
  }
  let val = (1.0 + f32(frac) / 1024.0) * pow(2.0, f32(i32(exp) - 15));
  return select(val, -val, sign == 1u);
}

@compute @workgroup_size(256)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
  let row = gid.x;
  if (row >= params.N) { return; }

  // Each Q6_K super-block = 210 bytes covering 256 weights along the K dimension.
  // For row n, the weight super-blocks for that row start at byte offset:
  //   row_byte_base = row * params.K_blocks * 210
  let row_byte_base = row * params.K_blocks * 210u;

  var acc: f32 = 0.0;
  for (var b: u32 = 0u; b < params.K_blocks; b = b + 1u) {
    let blk = row_byte_base + b * 210u;
    let ql_base     = blk;          // 128 bytes
    let qh_base     = blk + 128u;   //  64 bytes
    let scales_base = blk + 192u;   //  16 bytes (i8 each)
    let d_off       = blk + 208u;   //   2 bytes (f16)

    let d = read_d_f16_as_f32(d_off);
    let k_base = b * 256u;

    // Two halves of 128 elements each.
    for (var half_i: u32 = 0u; half_i < 2u; half_i = half_i + 1u) {
      // half 0: ql[0..64], qh[0..32], scales[0..8]
      // half 1: ql[64..128], qh[32..64], scales[8..16]
      let ql_half_off = ql_base + half_i * 64u;
      let qh_half_off = qh_base + half_i * 32u;
      let sc_half_off = scales_base + half_i * 8u;
      let half_k_off  = k_base + half_i * 128u;

      for (var l: u32 = 0u; l < 32u; l = l + 1u) {
        // four interleaved scale slots
        let s0 = f32(read_i8(sc_half_off + 0u));
        let s2 = f32(read_i8(sc_half_off + 2u));
        let s4 = f32(read_i8(sc_half_off + 4u));
        let s6 = f32(read_i8(sc_half_off + 6u));

        // ql byte holds two 4-bit nibbles for positions l (low) and l+32 (high)
        let ql_byte = read_u8(ql_half_off + l);
        let ql_lo = ql_byte & 0xFu;
        let ql_hi = (ql_byte >> 4u) & 0xFu;

        // qh byte holds 2-bit slices for positions l, l+32, l+64, l+96
        let qh_byte = read_u8(qh_half_off + (l & 31u));
        let qh0 = (qh_byte >> 0u) & 0x3u;
        let qh1 = (qh_byte >> 2u) & 0x3u;
        let qh2 = (qh_byte >> 4u) & 0x3u;
        let qh3 = (qh_byte >> 6u) & 0x3u;

        // q values (signed: subtract 32)
        let q0 = i32(ql_lo | (qh0 << 4u)) - 32;
        let q1 = i32(ql_hi | (qh1 << 4u)) - 32;
        // l+64 and l+96 use the same ql_byte at offset+32 in the half (different ql)
        let ql_byte2 = read_u8(ql_half_off + 32u + l);
        let ql_lo2 = ql_byte2 & 0xFu;
        let ql_hi2 = (ql_byte2 >> 4u) & 0xFu;
        let q2 = i32(ql_lo2 | (qh2 << 4u)) - 32;
        let q3 = i32(ql_hi2 | (qh3 << 4u)) - 32;

        let w0 = d * s0 * f32(q0);
        let w1 = d * s2 * f32(q1);
        let w2 = d * s4 * f32(q2);
        let w3 = d * s6 * f32(q3);

        acc = acc + w0 * input[half_k_off + l];
        acc = acc + w1 * input[half_k_off + l + 32u];
        acc = acc + w2 * input[half_k_off + l + 64u];
        acc = acc + w3 * input[half_k_off + l + 96u];
      }
    }
  }

  output[row] = acc;
}
```

The above structure is correct. Verify the indexing carefully against llama.cpp `dequantize_row_q6_K` and return ONLY the corrected complete .wgsl file (no markdown, no commentary). If you spot any indexing bug, fix it.'

ESC_PROMPT=$(printf '%s' "$PROMPT" | python3 -c "import sys,json; print(json.dumps(sys.stdin.read()))")

curl -s -m 600 -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "{\"prompt\": $ESC_PROMPT, \"max_length\": 3072, \"temperature\": 0.15, \"top_p\": 0.9, \"rep_pen\": 1.05}" \
  > "$OUT_DIR/fortytwo_q6k_fused_v2.json" 2>&1

python3 -c "import json,sys; d=json.load(open('$OUT_DIR/fortytwo_q6k_fused_v2.json')); print(d.get('results',[{}])[0].get('text',''))" > "$OUT_DIR/fortytwo_q6k_fused_v2.wgsl" 2>"$OUT_DIR/fortytwo_q6k_fused_v2.err"
echo "FortyTwo retry done: $(wc -l < "$OUT_DIR/fortytwo_q6k_fused_v2.wgsl") lines"
