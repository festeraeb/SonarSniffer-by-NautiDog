# CESAROPS Final Architecture — The Three-Brain Stack (Hot Swap)

## Hardware Assignment (LOCKED)

| Role | Model | Hardware | Node | Port |
|------|-------|----------|------|------|
| Main Worker | Qwen3.6-35B-A3B MoE MXFP4 | Dual P100 32GB HBM2 | T440 | 5001 |
| Researcher (Phase 1) | DeepSeek-R1-Distill-Qwen-32B Q4_K_M | Xeon DDR4 92GB (hot swap) | T440 Socket 1 | 5557 |
| Corrector (Phase 2) | Fortytwo_Strand-Rust-Coder-14B F16 (ZERO QUANT) | Xeon DDR4 92GB (replaces R1) | T440 Socket 1 | 5557 |
| Fast Fallback | Qwen2.5-Coder-14B Q4_K_M | 1070 8GB (cesarops2) | 100.102.158.111 | 5555 |
| Embeddings | nomic-embed-text-v1.5 (137M) | P1000/P106 (cesarops3) | 100.105.77.74 | 5558 |
| nautivecs search | Pure Rust (no model) | T440 CPU | T440 | 5003 |
| WSO (web search) | SearXNG proxy | T440 CPU | T440 | 5010 |
| Overseer | Kiro (Claude) | Cloud - ONLY when system fails | - | - |

## Hot Swap Strategy (Xeon DDR4 92GB time-share)

Phase 1 (Planning):
- R1-32B loads on Xeon Socket 1 (~20GB)
- Gets the FULL cesarops-inference spec
- Reasons deeply about improvements, weak spots, novel ideas
- Searches nautivecs + web for latest AI inference techniques
- Outputs: improved spec + ideas → saved to nautivecs
- UNLOAD R1. Its job is done.

Phase 2 (Building):
- Fortytwo_Strand-14B F16 loads on Xeon Socket 1 (~28GB, ZERO quantization)
- Becomes the permanent Rust auditor/corrector for the entire build
- Full FP16 precision = no quantization noise on complex Rust lifetime logic
- Watches the 35B's tool calls, fixes malformed JSON, nudges corrections
- Stays loaded until the build is complete

The 1070 on cesarops2 stays available as fast fallback during the swap window.

## Why F16 for the Corrector

- 14B params x 2 bytes = 28GB. Fits in 92GB with 64GB headroom for KV + OS.
- Zero quantization noise = bit-perfect to training weights
- On complex Rust code (lifetimes, generics, trait bounds), quantized models "stutter"
- F16 follows nested logic through crates without losing the thread
- AVX-512 on Xeon 4110 handles FP16-to-FP32 upcasting efficiently
- For a "Master Auditor" checking f64 curvelet math, you want full dynamic range

## Data Flow

```
[Phase 1: R1 Planning]
Full Spec → R1-32B (Xeon) → Deep reasoning + nautivecs search
    → Improved spec + ideas → nautivecs
    → UNLOAD R1

[Phase 2: Building]
Spec Chunk
    ↓
[Thinker pre-flight: nautivecs search for relevant code]
    ↓
35B MoE (P100s) → reasons about WHAT to do → attempts tool call
    ↓
    If tool call malformed:
        → Fortytwo_Strand F16 (Xeon) fixes JSON, executes tool
        → Passes result back WITH correction note
        → Saves correct format to nautivecs
        → "Here's your data. Do it right next time."
    ↓
    If tool call correct:
        → Execute directly, pass result back
    ↓
Loop continues (max 12 rounds)
    ↓
Response / File written / cargo check
```

## The Creativity-Grounding Split

| Layer | Temperature | Role |
|-------|-------------|------|
| R1 (Phase 1 only) | 0.8 | Creative researcher - novel ideas, unexplored angles |
| 35B MoE (P100s) | 0.4-0.6 | Main worker - strategy + execution |
| Strand F16 (Xeon) | 0.1 | Zero-creativity auditor - pure syntax, pure accuracy |
| nautivecs | N/A | Facts. 12,600+ chunks. Makes any model a 400B specialist. |

## Services

### Phase 1 service (R1 researcher):
```
numactl --cpunodebind=1 --membind=1 koboldcpp \
    --model /codebase/models/DeepSeek-R1-Distill-Qwen-32B-Q4_K_M.gguf \
    --port 5557 --threads 8 --contextsize 8192 --usecublas 0
```

### Phase 2 service (Strand corrector):
```
numactl --cpunodebind=1 --membind=1 koboldcpp \
    --model /codebase/models/Fortytwo_Strand-Rust-Coder-14B-v1-F16.gguf \
    --port 5557 --threads 8 --contextsize 16384 --usecublas 0
```

### Swap procedure:
1. Kill Phase 1 process: `pkill -f DeepSeek-R1`
2. Wait 3s for RAM to free
3. Start Phase 2 process with Strand F16
4. Verify health on :5557

## Downloads In Progress

- DeepSeek-R1-Distill-Qwen-32B-Q4_K_M.gguf (~20GB) → T440 /codebase/models/
- Fortytwo_Strand-Rust-Coder-14B-v1-F16.gguf (~28GB) → T440 /codebase/models/
- nomic-embed-text-v1.5.Q8_0.gguf (140MB) → cesarops2 (transfer to cesarops3)
