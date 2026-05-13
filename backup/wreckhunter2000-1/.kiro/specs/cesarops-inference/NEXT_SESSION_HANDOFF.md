# NEXT SESSION — Fix Gibberish Output from cesarops-inference

## STATUS: Attention bias patch applied, STILL GIBBERISH — move to Q6_K dequant

## SESSION UPDATE (May 13, 2026 — Kiro):

### What was done this session:
1. **Applied attention bias fix** to transformer.rs — added Q/K/V bias vectors after matmul, before RoPE
2. **Tripled forge-v2 token caps** (300→900, 4096→12288, 200→600, 500→1500) in loop_engine.rs and diagnostics.rs
3. **Confirmed model loads** on port 5005 with bias tensors visible in GGUF (blk.N.attn_q/k/v.bias)
4. **Tested output** — still gibberish: "OMX)didReceiveMemoryWarningServiçouple )ĊĊĊĊĊĊĊĊtubeboarding NTN@storeIOR"
5. **Set up SSH config** for VS Code Remote to T440 (Host: t440, 100.72.182.77, user: cesarops)

### What this means:
- The bias fix was necessary but NOT sufficient
- The forward pass computes something wrong BEFORE the bias matters
- **Primary suspect is now Q6_K dequantization** — the bit-extraction order may be wrong
- Secondary: verify the bias patch actually compiled into the running binary (Syncthing may not have synced transformer.rs to T440 before the build)

### IMMEDIATE NEXT STEPS:
1. **Verify the bias patch is in the running binary** — grep transformer.rs on T440 for "attn_q.bias"
2. **If patch IS there → Q6_K dequant is the bug** — compare our dequant against llama.cpp reference
3. **If patch is NOT there → rebuild with patch** and retest
4. **Remote development** — VS Code Remote SSH to t440 is configured, open /codebase/repos/wreckhunter2000-1/ (or /home/cesarops/wreckhunter2000-1/ if /codebase doesn't show in file picker)

### Path confusion:
- Build/run path on T440: `/codebase/repos/wreckhunter2000-1/cesarops-inference/`
- `/codebase/` may be a symlink or mount — verify with `ls -la /codebase`
- Syncthing mirror: `/home/cesarops/wreckhunter2000-1/`
- Model: `/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf`

### Port layout (current):
- 5001: KoboldCPP (Qwen3.6-35B, production, don't touch)
- 5002: Something already bound (old cesarops-inference instance?)
- 5005: cesarops-inference test instance (1.5B Q6_K, bias patch — gibberish output)
- 9100: forge-v2 web UI

The engine runs end-to-end on T440 with the 1.5B Q6_K model. All mechanical issues are solved.
The output is multilingual garbage instead of coherent English.

## WHAT WORKS (proven on T440, compiles + runs):

- GGUF v3 loader with **data_offset alignment** (critical bug fix this session)
- Q6_K dequantization (quant_type 14) — values in correct range
- Real BPE tokenizer from tokenizer.json (151,665 vocab, 151,387 merges)
- KV cache (stores K/V per layer per position, multi-token attention)
- Full 28-layer forward pass: RMSNorm → Q/K/V → RoPE → GQA attention → O proj → SwiGLU FFN
- Sampling (temperature + top_p + rep_pen + banned_tokens)
- KoboldCPP-compatible HTTP API on port 5002
- ~13-18 sec/token on CPU (Xeon 4110, naive matmul)

## THE REMAINING BUG: Gibberish output

Output examples: "électriqueergus piger", "cảnh)did AssemblyCompany", "觢 pakistan ?>:"

The logits have healthy ranges (min=-18, max=+17), no NaN, different token IDs each step.
This means the forward pass computes *something* but not the *correct* thing.

## SUSPECTS (in priority order):

### 1. ATTENTION BIAS TENSORS (HIGH PRIORITY)
The GGUF contains `blk.{N}.attn_q.bias`, `blk.{N}.attn_k.bias`, `blk.{N}.attn_v.bias`.
**We are NOT applying these.** Qwen2.5 uses bias in Q/K/V projections.
After the matmul, we need: `q = q_weight × hidden_state + q_bias`
This is likely the primary cause of gibberish — missing bias shifts the projections.

### 2. Q6_K DEQUANTIZATION ACCURACY
The Q6_K implementation may have subtle bit-extraction errors in the ql/qh interleaving.
llama.cpp's actual Q6_K layout processes elements in a specific interleaved order for SIMD,
not the simple sequential order we use. Reference: ggml-quants.c `dequantize_row_q6_K`.

### 3. TOKENIZER PRE-TOKENIZATION
Our BPE pre-tokenizer splits on spaces/punctuation and uses Ġ (U+0120) for space prefix.
Qwen's actual pre-tokenizer uses a complex regex pattern. "Hello" → 9707 seems correct,
but multi-word prompts may tokenize differently than expected.

### 4. ROPE THETA / FREQUENCY BASE
We use rope_theta=1000000.0. Qwen2.5-1.5B might use a different value.
Check GGUF metadata key `qwen2.rope.freq_base`.

## ARCHITECTURE CONFIRMED:
- 28 layers, 1536 hidden, 12 heads, 2 kv_heads, head_dim=128
- intermediate_size=8960 (derived from ffn_gate tensor shape)
- vocab_size=152064 (metadata), embedding has 151936 columns
- GQA: 6 query heads per KV head

## GGUF TENSOR NAMING (llama.cpp format):
- `token_embd.weight` — embedding [1536, 151936] Q6_K
- `blk.{N}.attn_norm.weight` — pre-attention RMSNorm [1536] F32
- `blk.{N}.attn_q.weight` — Q projection [1536, 1536] Q6_K
- `blk.{N}.attn_q.bias` — Q bias [1536] F32 ← NOT APPLIED YET
- `blk.{N}.attn_k.weight` — K projection [1536, 256] Q6_K
- `blk.{N}.attn_k.bias` — K bias [256] F32 ← NOT APPLIED YET
- `blk.{N}.attn_v.weight` — V projection [1536, 256] Q6_K
- `blk.{N}.attn_v.bias` — V bias [256] F32 ← NOT APPLIED YET
- `blk.{N}.attn_output.weight` — O projection [?, ?] Q6_K
- `blk.{N}.ffn_norm.weight` — post-attention RMSNorm [1536] F32
- `blk.{N}.ffn_gate.weight` — gate proj [1536, 8960] Q6_K
- `blk.{N}.ffn_up.weight` — up proj [1536, 8960] Q6_K
- `blk.{N}.ffn_down.weight` — down proj [8960, 1536] Q6_K
- `output_norm.weight` — final RMSNorm [1536] F32
- `output.weight` — lm_head [1536, 151936] Q6_K

## GGUF DIMENSION CONVENTION:
GGUF is column-major. Shape [A, B] means A is the contiguous stride dimension.
- Embedding [1536, 151936]: token t's embedding = data[t*1536 .. (t+1)*1536]
- Weight matrices: currently using matmul_f32_transposed_b for projections
- The transpose vs non-transpose produced IDENTICAL results for square matrices
- For non-square (K, gate, up, down), need to verify orientation empirically

## MATMUL CONVENTION:
- `matmul_f32(a, b, m, k, n)`: A[m,k] × B[k,n] → C[m,n] (standard row-major)
- `matmul_f32_transposed_b(a, b_t, m, k, n)`: A[m,k] × B_T[n,k]^T → C[m,n]
- Currently using transposed_b for all weight projections

## FILES ON T440 (at /codebase/repos/wreckhunter2000-1/cesarops-inference/):
All source files are current via SCP. Build with:
```bash
source ~/.cargo/env
cd /codebase/repos/wreckhunter2000-1/cesarops-inference
cargo build --release
```

## TEST COMMANDS:
```bash
# Full rebuild + test cycle:
bash /tmp/deploy_and_test.sh

# Just test (if server already running on 5002):
bash /tmp/test_inference.sh

# Longer prompt test:
curl -s -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt":"The capital of France is","max_length":10,"temperature":0.3}'
```

## IMMEDIATE NEXT STEPS:

1. **Add attention bias** — after Q/K/V matmul, add the bias vectors:
   ```rust
   let q_bias = self.load_tensor_f32(weights, &format!("blk.{}.attn_q.bias", layer_idx), q_dim);
   for i in 0..q_dim { q[i] += q_bias[i]; }
   ```

2. **Verify Q6_K against reference** — compare our dequant output for a known tensor
   against llama.cpp's output (or use the F32 norm weights as a sanity check since those
   don't go through Q6_K)

3. **Check rope_theta from metadata** — read `qwen2.rope.freq_base` from GGUF metadata

4. **Remove diagnostic logging** — the FFN shape logs and logit range logs slow things down

## CLUSTER ACCESS:
- T440: ssh cesarops@100.72.182.77
- Models: /codebase/models/
- Repo: /codebase/repos/wreckhunter2000-1/
- cargo: source ~/.cargo/env

## DO NOT:
- Rewrite transformer.rs from scratch
- Reference WgpuFlashAttention, wgpu::Device, dispatch_attention (don't exist)
- Use f64 in the forward pass (it's all f32)
- Use bytemuck::cast_slice between f32/f64
- Change the matmul function signatures
