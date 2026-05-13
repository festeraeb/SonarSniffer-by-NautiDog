# R1 + Kiro + Gemini Research — Consolidated Findings

## Section 1: Risks (R1 Assessment)
- Burn/wgpu maturity on Pascal — valid, mitigated by custom shaders
- Zero-copy across memory tiers — complex but GridBuffer handles it
- MoE sharding — load balancing needs care
- Concurrency — NOT an issue (mode-switched, not simultaneous)

## Section 2: Hardware Strengths
- 4-tier memory hierarchy (32GB HBM2 + 8GB GDDR5 + 92GB DDR4 + 4TB RAID)
- RAID as "Frozen KV" — infinite context via mmap, validated by FlexGen/InstInfer research
- Dual-socket NUMA — 128GB/s aggregate DDR4, independent memory controllers
- DDR4 as prefetch buffer eliminates RAID→GPU bandwidth bottleneck (IBM 2025, NeurIPS 2025)
- For SAR: 1 tok/s is fine if it means "remembering" 20 days of data

## Section 3: Cutting-Edge Techniques (2024-2026)
- **Speculative decoding**: 1070 as draft model (1.5B), P100s verify (2-3x speedup)
- **Self-speculative decoding**: MoE early layers as draft (no separate model)
- **Disaggregated inference**: P100s prefill, 1070/P106 decode (prevents memory wall)
- **CubeCL/CubeK (Burn 0.20, Jan 2026)**: Unified CPU/GPU kernels, 5x faster
- **TierKV / Mooncake**: SSD-based KV for agentic trajectories (20-day stacks)
- **Register-heavy kernels**: Load 16x16 tiles into P100's 256KB register file, bypass HBM2 bus

## Section 4: Rust Crates & Tools
- **wgpu-llm-cli** — wgpu-based LLM inference, NO CUDA required (our exact approach!)
- **llama-gguf** — Pure Rust GGUF loader with full format support
- **burn-wgpu** — Burn's WGPU backend (our target framework)
- **candle-vllm** — Efficient serving with OpenAI-compatible API
- **model-rs** — HF model download + Candle inference
- **nauticuvs** — OUR crate, f64 curvelet transform (already built)
- **scirs2-signal** — Matched filtering + CFAR detection for sonar (need to verify)
- **mistral-rs** — Pure Rust inference on Candle, blazing fast

## Section 5: Performance Expectations
- 35B MoE (3B active params per token) on dual P100s:
  - Memory-bound at HBM2 bandwidth (732 GB/s per card)
  - Expected: 15-25 tok/s for generation (matching KoboldCPP baseline)
  - With speculative decoding (1070 draft): potentially 30-50 tok/s
  - Prefill: limited by compute (19 TFLOPS FP16 per card = 38 TFLOPS total)
- FlexGen proved 1 tok/s on 175B with single 16GB GPU + disk
- Our setup is 10x richer than FlexGen's test hardware

## Key Decisions from Research
1. Use `wgpu-llm-cli` as reference (don't reinvent the wheel)
2. Use `llama-gguf` for GGUF parsing (don't write our own)
3. Build on `burn-wgpu` for the compute backend
4. Implement speculative decoding with 1070 as draft model
5. Register-heavy 16x16 tile kernels for the matmul inner loop
6. CubeCL when Burn 0.20 is stable enough
7. RAID mmap via `memmap2` crate for Frozen KV tier
