#!/bin/bash
# Dispatch to DeepSeek-R1-7B on cesarops3 (10.0.0.41:5100).
# Task #3: Fused QKV projection + RoPE + KV cache write.
# Currently 3 separate submits (S2 QKV, S3 biases, S4 RoPE+KV write).
# Goal: collapse the RoPE+KV-write into the QKV projection itself.
#
# This is research-flavored — let R1 think hard about it.
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r6_scout_qkv_rope.prompt" << 'PROMPT_EOF'
You are a transformer kernel design expert. The cesarops-inference engine on
Pascal P100 currently does QKV projection + bias + RoPE + KV-cache-write as
THREE separate compute submits per layer:
  S2: q_proj, k_proj, v_proj (3 matvec dispatches in one submit)
  S3: q_bias add, k_bias add, v_bias add (in-place)
  S4: RoPE on Q, RoPE on K, copy K and V into the cache buffer at pos*kv_stride

Goal: collapse RoPE and the KV cache write into the QKV projection so per
layer we drop S4 entirely. Output a DESIGN PROPOSAL (markdown — short, terse).

CONSTRAINTS:
- Stay in WGSL via wgpu. Pascal Vulkan is the target.
- Single-token decode (m=1 case in dispatch_tiled_matmul). Prefill goes through
  the same path one position at a time.
- KV cache layout: [pos][n_kv_heads][head_dim], stride = n_kv_heads * head_dim.
  K cache write at pos p, kv_head h, dim j has byte offset:
  p * kv_stride * 4 + h * head_dim * 4 + j * 4.
- RoPE: half-split rotation on q and k only (not v). For a head of size hd,
  pair (j, j+hd/2) gets rotated by angle pos / theta^(2j/hd).
- v_proj has bias too, but no RoPE; v writes straight to cache.
- We already have matvec_pc, matvec_vec4_pc, matvec_bias, matvec_bias_vec4_pc
  pipelines. Push constants are available (Pascal supports it).

QUESTIONS TO ANSWER IN THE DESIGN:
1) Is it cleaner to (a) extend matvec_bias_vec4_pc to optionally apply RoPE
   and write directly to cache, or (b) write a NEW shader matvec_qkv_rope_pc
   that does the full QKV+bias+RoPE+cache-write for one head per dispatch?
2) Where does the n-kv-head replication (head 0 of a kv_head feeds n_heads/n_kv_heads
   query heads) live? In the kernel, or do we still launch separate q/k/v dispatches?
3) Does fusing change correctness for the existing diagnostic readback at
   pos=1 head=0 (scores=[181.962, 183.605])? Define how to verify parity.
4) Push constant size: current cap is 64 bytes. Params now include pos,
   head_dim, kv_stride, kv_head_offset, theta, plus an enum for which projection
   (Q/K/V). Does it still fit?
5) What's the rollback path if it regresses on the 1070 cross-card smoke?

Format the answer as:
  ## Recommendation
  ## Shader interface
  ## Dispatch flow
  ## Verification plan
  ## Risks / rollback

Be specific. Real WGSL bindings, real struct layout. No fluff. Maximum 200 lines.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r6_scout_qkv_rope.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 3500, 'temperature': 0.4, 'top_p': 0.9, 'rep_pen': 1.05}))")

curl -s -m 1500 -X POST http://10.0.0.41:5100/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r6_scout_qkv_rope.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r6_scout_qkv_rope.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r6_scout_qkv_rope.md" 2>"$OUT_DIR/r6_scout_qkv_rope.err"
echo "[Scout (R1-7B) r6] qkv_rope_fused design: $(wc -l < "$OUT_DIR/r6_scout_qkv_rope.md") lines"
