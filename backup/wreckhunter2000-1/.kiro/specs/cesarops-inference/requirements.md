# cesarops-inference: Native Rust LLM Inference Engine

## Vision

Replace KoboldCPP with a pure Rust inference engine built on Burn + warp-grid's unified memory. Zero PCIe hops between inference and compute. The same HBM2 that runs dipole detection also runs token generation. Hardware-interrogating — adapts to whatever silicon it finds.

## Requirements

### R1: GGUF Model Loading into GridBuffer

- Load GGUF model files from RAID (/codebase/models/*.gguf)
- Map weights into GridBuffer handles (not raw Vec<u8>)
- Support MXFP4, Q4_K_M, Q8_0 quantization formats
- MoE routing: shard experts across GPUs (P100 #0 gets experts 1-8, P100 #1 gets 9-16)
- Weight migration: GridBuffer::from_raid_file() → migrate(Gpu) on startup

### R2: GridBuffer ↔ Burn Tensor Bridge

- Zero-copy conversion: GridBuffer.as_burn_tensor() wraps existing wgpu::Buffer
- No re-allocation on HBM2 — Burn sees the same memory the scan pipeline uses
- Precision-aware: if GridBuffer is f16, Burn gets an f16 tensor (no upcast)
- Bidirectional: Burn tensor results can be stored back as GridBuffer for pipeline use

### R3: Transformer Forward Pass via Burn

- Universal transformer template — not hardcoded to Qwen
- Takes a ModelSpec that defines: layer count, expert count, head count, hidden dim
- Uses custom matmul_half2.wgsl for P100 linear layers (2:1 FP16 throughput)
- Falls back to matmul_f32.wgsl on SM 6.1 cards (1070, P1000)
- Attention: standard multi-head attention with rotary embeddings
- MoE routing: top-k expert selection, cross-GPU dispatch via GridBuffer::migrate()

### R4: KV Cache as GridBuffer

- KV cache lives in HBM2 (Tier 0) during active inference
- When context exceeds GPU capacity: overflow to NUMA-pinned DDR4 (Tier 1)
- GridBuffer::migrate(Host { numa_node }) handles the overflow automatically
- Ring buffer semantics: oldest KV heads evicted first
- NUMA-aware: Socket 0's DDR4 holds overflow for P100 #0's cache

### R5: Tokenizer

- Rust-native tokenizer for Qwen models (tiktoken-rs or HuggingFace tokenizers crate)
- Encode: String → Vec<u32> token IDs
- Decode: Vec<u32> → String
- Special tokens: <|im_start|>, <|im_end|>, <tool_call>, </tool_call>

### R6: Sampling

- Temperature scaling
- Top-p (nucleus) sampling
- Repetition penalty with configurable range
- Stop sequences: ["</tool_call>", "<|im_end|>"]
- All in Rust — no external dependencies

### R7: HTTP API (KoboldCPP-compatible)

- POST /api/v1/generate — same JSON format as KoboldCPP
- Request: {"prompt", "max_length", "temperature", "top_p", "rep_pen", "stop_sequence"}
- Response: {"results": [{"text": "..."}]}
- Drop-in replacement — forge-web doesn't need any changes
- Also expose: GET /api/v1/model, GET /health

### R8: Hardware Interrogation at Startup

- Detect all GPUs via wgpu enumerate_adapters()
- Classify by capability (not brand): supports_f16, vram_total, bandwidth
- Build a ModelSpec that matches the hardware:
  - 2x P100 (32GB total, f16 2:1) → tensor parallel, half2 kernels
  - 1x 1070 (8GB, no f16 benefit) → small model, f32 kernels
  - 1x P106 (6GB) → tiny model or embedding only
- Log the topology and save to nautivecs via remember tool

### R9: Concurrent Inference + Compute

- The wgpu Device is shared between inference and scan pipeline
- When a scan is triggered during inference: partition workgroups
- Or: time-slice (inference pauses during scan dispatch, resumes after)
- The GridBuffer handles both model weights AND scan tiles in the same HBM2 pool
- No data movement needed to switch between modes

## Architecture

```
cesarops-inference/
├── Cargo.toml
├── src/
│   ├── lib.rs              # Public API
│   ├── loader.rs           # GGUF → GridBuffer weight loading
│   ├── bridge.rs           # GridBuffer ↔ Burn Tensor zero-copy bridge
│   ├── transformer.rs      # Universal transformer forward pass
│   ├── attention.rs        # Multi-head attention with RoPE
│   ├── moe.rs              # Mixture of Experts routing + cross-GPU dispatch
│   ├── kv_cache.rs         # KV cache with GridBuffer overflow to DDR4
│   ├── tokenizer.rs        # Qwen tokenizer (encode/decode)
│   ├── sampling.rs         # Temperature, top_p, rep_pen, stop sequences
│   ├── server.rs           # Axum HTTP API (KoboldCPP-compatible)
│   └── hardware.rs         # Hardware interrogation + ModelSpec generation
```

## Dependencies

```toml
[dependencies]
burn = { version = "0.16", features = ["wgpu"] }
burn-wgpu = "0.16"
tokenizers = "0.20"
warp-grid = { path = "../warp-grid" }
axum = "0.8"
tokio = { version = "1", features = ["full"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
half = "2"
bytemuck = { version = "1", features = ["derive"] }
```

## Universal Device Abstraction

```rust
pub enum BackendType {
    Wgpu,      // Modern GPU (P100, V100, A100, M10)
    Avx512,    // High-performance CPU (Xeon)
    Fallback,  // Standard CPU / any unknown hardware
}

pub struct UniversalDevice {
    pub id: usize,
    pub backend: BackendType,
    pub vram_total: u64,
    pub supports_f16: bool,
    pub supports_tensor_cores: bool,
    pub bandwidth_gbps: f32,
    pub numa_node: Option<u32>,
}
```

## The VIC-20 Test

If this engine runs on a system with:
- No GPU (BackendType::Fallback)
- 64KB RAM
- No f16 support

It should still work — just slowly. The KV cache overflows to disk (Mapped), the matmul uses scalar f32, and inference happens one token at a time. The architecture doesn't break — it just scales down gracefully.

## Success Criteria

- Loads Qwen3.6-35B-A3B MXFP4 across both P100s
- Generates tokens at >= 15 tok/s (matching KoboldCPP baseline)
- forge-web works without any changes (same API)
- Scan pipeline can run concurrently without data movement
- KV cache overflows gracefully to DDR4 at 32K+ context
- The same binary runs on cesarops2 (1070) with a smaller model
