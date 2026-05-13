# LLM Inference Benchmark Results — 2026-05-07

## Hardware Tested

| Node | GPU(s) | CPU | RAM |
|------|--------|-----|-----|
| cesarops2 | GTX 1070 8GB + Quadro P1000 4GB | Xeon E3-1265L v3 (4c/8t) | 32GB |
| T440 | 2× Tesla P100-PCIE-16GB | Dual Xeon Silver (94GB) | 94GB |

## Benchmark Results

### Single-GPU Inference (cesarops2 — GTX 1070)

| Engine | Model | Format | tok/s | Notes |
|--------|-------|--------|-------|-------|
| **KoboldCPP (CUDA)** | TinyLlama 1.1B | Q4_K_M GGUF | **94.7** | Baseline king |
| **KoboldCPP (CUDA)** | Qwen2.5-1.5B-Instruct | Q4_K_M GGUF | **45.6** | Good for workers |
| wgpu-llm (Vulkan) | TinyLlama 1.1B | FP16 safetensors | 26.0 | Portable, no CUDA needed |
| Cake (Vulkan) | Qwen2.5-1.5B-Instruct | FP16 safetensors | 4.46 | UMA spill to RAM |
| Cake (Vulkan) | Qwen2.5-Coder-3B | FP16 safetensors | 2.18 | UMA spill, segfault on exit |

### Multi-GPU Inference (T440 — Dual P100)

| Engine | Model | Config | Status |
|--------|-------|--------|--------|
| **KoboldCPP (CUDA)** | Qwen2.5-Coder-14B | Q4_K_M, tensor_split 3:1 | **LIVE via Cloudflare tunnel** |

### Failed/Dropped

| Engine | Reason |
|--------|--------|
| Crane | Only supports Qwen architecture, didn't detect GPU (CPU-only) |
| mistral.rs | CUDA atomicAdd error on Pascal GPUs, CPU fallback too slow |
| Cake (BF16) | Pascal GPUs don't support BF16 matmul |

## Key Findings

1. **KoboldCPP with GGUF quantization is unbeatable on Pascal GPUs** — 10-20× faster than Vulkan alternatives because Q4_K_M fits entirely in VRAM with zero PCIe traffic.

2. **Cake's value is distribution, not speed** — on single-node Pascal hardware, it's 10× slower than KoboldCPP. Its advantage is sharding models across multiple machines over the network (for models too big for one GPU).

3. **wgpu-llm at 26 tok/s is viable for field deployment** — no CUDA dependency, runs on any Vulkan GPU. Acceptable for structured decisions (5s per 128-token response).

4. **Model quality matters more than size** — Qwen2.5-1.5B followed grounding rules perfectly while TinyLlama 1.1B hallucinated wildly. Same parameter ballpark, completely different capability.

5. **The 14B model on dual P100s via Cloudflare tunnel** is the production setup — accessible from anywhere at `https://llm.cesarops.org/v1`.

## Steering Test Results

| Model | Unsteered | Steered | Correction Respected |
|-------|-----------|---------|---------------------|
| TinyLlama 1.1B | Full hallucination | Partial grounding (still invents) | ❌ Ignored correction |
| Qwen2.5-1.5B | Hallucinated function names | ✅ Cited real code, exact values | ✅ Used 0.3 override |

## Architecture Decision

- **Primary inference**: KoboldCPP + GGUF on T440 P100s (14B model, 32K context)
- **Backup/field**: KoboldCPP on cesarops2 1070 (1.5B-3B models)
- **Future distributed**: Cake for models >16GB that need multi-node sharding
- **Long-term custom**: wgpu-llm shaders integrated into cesarops-hybrid-engine (see whitepaper)

## Live Endpoints

| URL | Service | Model |
|-----|---------|-------|
| `https://llm.cesarops.org/v1` | KoboldCPP (OpenAI-compat) | Qwen2.5-Coder-14B Q4_K_M |
| `https://api.cesarops.org` | wrecks-api (when running) | — |
| `https://app.cesarops.org` | Frontend (cesarops3 backup) | — |

## Hardware Status

| Component | Status |
|-----------|--------|
| T440 P100 GPU 0 | ✅ Online, 34°C, healthy |
| T440 P100 GPU 1 | ✅ Online, 32°C, healthy (different riser — no issues) |
| T440 Coral TPU | ⚠️ PCIe detected, gasket driver incompatible with kernel 6.17 |
| Cloudflare tunnel | ✅ Active, all subdomains routing |
| cesarops2 (1070+P1000) | ✅ Online, Cockpit at :9090 |
| cesarops3 (1060) | ✅ Online, backup frontend serving |
