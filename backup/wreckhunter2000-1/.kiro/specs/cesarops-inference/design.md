# cesarops-inference: Technical Design

## Hardware Archaeologist (First Boot)

On startup, the engine runs `audit_system()` which produces an `IronProfile`:

```rust
pub struct IronProfile {
    pub gpu_nodes: Vec<GpuNode>,      // From warp-grid::pool (wgpu enumeration)
    pub cpu_nodes: Vec<CpuNode>,      // AVX-512 detection, core count
    pub numa_topology: NumaTopology,  // From warp-grid::numa (/sys filesystem)
    pub total_vram_mb: u64,           // Sum across all GPUs
    pub total_host_ram_mb: u64,       // From /proc/meminfo
    pub raid_available_gb: u64,       // From df on /codebase
}

pub struct CpuNode {
    pub socket_id: u32,
    pub core_count: u32,
    pub has_avx512: bool,
    pub ddr_bandwidth_gbps: f32,
}
```

The IronProfile determines:
- How to shard the model (which experts go where)
- Whether to use f16 or f32 kernels
- How much KV cache fits in VRAM vs DDR4
- NUMA pinning rules for inference threads

## GGUF Loader → GridBuffer

```rust
pub struct GgufLoader {
    path: PathBuf,
    profile: IronProfile,
}

impl GgufLoader {
    /// Maps GGUF tensors directly into GridBuffers.
    /// Shards across GPUs based on IronProfile.
    pub async fn load(&self) -> Result<ModelWeights, Error> {
        // 1. Memory-map the GGUF file (GridBuffer::from_raid_file)
        // 2. Parse tensor metadata (names, shapes, quantization)
        // 3. For each tensor:
        //    - If it fits on one GPU: migrate(Gpu { gpu_index })
        //    - If too large: shard across GPUs (tensor parallelism)
        //    - MoE experts: round-robin across available GPUs
        // 4. Return ModelWeights with all tensors as GridBuffers
    }
}

pub struct ModelWeights {
    pub layers: Vec<LayerWeights>,
    pub embedding: GridBuffer,
    pub lm_head: GridBuffer,
}

pub struct LayerWeights {
    pub attention_qkv: GridBuffer,
    pub attention_out: GridBuffer,
    pub ffn_gate: GridBuffer,      // MoE: one per expert
    pub ffn_up: GridBuffer,        // MoE: one per expert
    pub ffn_down: GridBuffer,      // MoE: one per expert
    pub norm: GridBuffer,
    pub device: DeviceLocation,    // Which GPU this layer lives on
}
```

## GridBuffer ↔ Burn Tensor Bridge

Zero-copy conversion — Burn sees the same HBM2 memory:

```rust
impl GridBuffer {
    /// Wraps existing wgpu::Buffer into a Burn Tensor without re-allocation.
    pub fn as_burn_tensor<B: burn::backend::Backend>(
        &self,
        device: &B::Device
    ) -> burn::tensor::Tensor<B, 1> {
        match &self.storage {
            BufferStorage::GpuBuffer(wgpu_buf) => {
                // Zero-copy: Burn wraps our existing buffer
                burn::tensor::Tensor::from_primitive(
                    burn_wgpu::WgpuTensor::from_existing(wgpu_buf.clone(), self.shape.clone())
                )
            }
            _ => panic!("Call migrate(Gpu) before as_burn_tensor"),
        }
    }
}
```

## Transformer Forward Pass

```rust
pub struct UniversalTransformer {
    pub layers: Vec<TransformerLayer>,
    pub kv_cache: KvCacheManager,
    pub profile: IronProfile,
}

impl UniversalTransformer {
    pub async fn forward(&mut self, tokens: &[u32], position: usize) -> Vec<f32> {
        // 1. Embed tokens
        // 2. For each layer:
        //    a. Attention (using KV cache in GridBuffer)
        //    b. If MoE: route to experts (cross-GPU via migrate())
        //    c. FFN with matmul_half2.wgsl on P100 or matmul_f32.wgsl on 1070
        // 3. Final norm + lm_head
        // 4. Return logits
    }
}
```

## KV Cache Overflow

```rust
pub struct KvCacheManager {
    /// Active cache in HBM2 (fast)
    gpu_cache: Vec<GridBuffer>,
    /// Overflow in DDR4 (NUMA-pinned, slower but huge)
    host_overflow: Vec<GridBuffer>,
    /// Max tokens before overflow triggers
    gpu_capacity_tokens: usize,
}

impl KvCacheManager {
    pub fn push(&mut self, key: GridBuffer, value: GridBuffer) {
        if self.current_tokens() >= self.gpu_capacity_tokens {
            // Overflow oldest entries to host DDR4
            let oldest_k = self.gpu_cache.remove(0);
            let oldest_v = self.gpu_cache.remove(0);
            oldest_k.migrate(DeviceLocation::Host { numa_node: 0 });
            oldest_v.migrate(DeviceLocation::Host { numa_node: 0 });
            self.host_overflow.push(oldest_k);
            self.host_overflow.push(oldest_v);
        }
        self.gpu_cache.push(key);
        self.gpu_cache.push(value);
    }
}
```

## Mode Switching (Inference ↔ Scan)

The wgpu Device is shared. When a scan is triggered:

```rust
pub enum EngineMode {
    Inference,           // LLM generating tokens
    Scan,               // Dipole detection / curvelet
    Concurrent(f32),    // Split: 70% inference, 30% scan (workgroup partition)
}

impl ForgeInferenceEngine {
    pub fn switch_mode(&mut self, mode: EngineMode) {
        match mode {
            EngineMode::Scan => {
                // Pause inference, release workgroups for scan pipeline
                // KV cache stays in HBM2 (no eviction needed)
            }
            EngineMode::Concurrent(split) => {
                // Partition: inference gets (split * total_workgroups)
                // Scan gets the rest
            }
            EngineMode::Inference => {
                // Full GPU for token generation
            }
        }
    }
}
```

## Why This Beats KoboldCPP

| Feature | KoboldCPP | cesarops-inference |
|---------|-----------|-------------------|
| Language | C++ (llama.cpp) | Pure Rust (Burn) |
| Memory | Separate process, own allocations | Shared GridBuffer pool |
| PCIe hops | Model ↔ API ↔ Forge (3 hops) | Zero (same wgpu Device) |
| FP16 | Generic CUDA half | Custom matmul_half2.wgsl (2:1 P100) |
| KV overflow | Drops context | Migrates to NUMA-pinned DDR4 |
| Multi-GPU | Basic tensor split | NUMA-aware, expert-level sharding |
| Scan integration | Impossible (separate process) | Same HBM2 pool, mode switching |
| Hardware agnostic | CUDA only | wgpu (Vulkan/Metal/DX12/WebGPU) |

## Implementation Order

1. `hardware.rs` — IronProfile audit (reuse existing numa.rs + metrics.rs)
2. `loader.rs` — GGUF parser + GridBuffer mapping
3. `bridge.rs` — GridBuffer ↔ Burn Tensor zero-copy
4. `attention.rs` — Multi-head attention with RoPE
5. `transformer.rs` — Forward pass using Burn
6. `moe.rs` — Expert routing + cross-GPU dispatch
7. `kv_cache.rs` — Overflow management
8. `sampling.rs` — Temperature, top_p, rep_pen
9. `tokenizer.rs` — Qwen tokenizer
10. `server.rs` — Axum API (drop-in KoboldCPP replacement)

## Gemini Research Additions (May 2026)

### Multi-Head Latent Attention (MLA)
DeepSeek V3/V4 introduced MLA to reduce KV cache requirements by ~90%.
Instead of storing full K/V heads, compress into a latent vector.
This makes the "Infinite Context" on the T440 even more stable.

Implementation: Add a `latent_compress()` step to `GridBuffer::migrate()`.
Before shipping KV heads over QUIC to the 1070, compress them into latent vectors.
Reduces network traffic by ~4x, making the Cake loop virtually invisible to the 35B.

### INT8 KV Quantization (tierKV pattern)
Evicted KV blocks (Tier 1/2) can be quantized to INT8 before storage.
Shrinks the cold cache by ~4x with minimal quality loss.
Only decompress back to FP16 when pulled into Tier 0 (HBM2) for active attention.

### Grammar-Constrained Sampler
Force the 35B to emit ONLY valid JSON tool calls during action phase.
Implement as a finite-state machine that masks invalid tokens at each step.
Combined with logit bias on <think>/<\/think>, the model literally cannot loop.

### pmetal-gguf (v0.4.0)
Use for zero-init weight mapping from RAID.
mmap the GGUF file → weights appear in virtual memory → only fault in pages as needed.
Model "starts thinking" in milliseconds because only active expert weights get paged in.

### Burn Compilation Note
Use `#![recursion_limit = "256"]` in lib.rs to prevent type nesting compilation errors
common with complex Burn tensor operations.

## MoE Review Additions (May 2026 — Qwen3.6-35B Assessment)

### Validated Additions to Implement:

#### 1. FlashAttention in WGSL
Standard attention is O(n²) and will bottleneck at long contexts.
Implement FlashAttention tiling pattern in WGSL:
- Tile Q, K, V into blocks that fit in shared memory
- Compute attention per-block without materializing full attention matrix
- Reduces memory from O(n²) to O(n) with clever tiling
- Critical for 32K+ context on P100 HBM2

#### 2. INT8 KV Cache Quantization (Cold Tiers)
KV cache in Tier 1 (DDR4) and Tier 2 (RAID) can be stored at INT8.
Doubles effective context window with minimal quality loss.
Only decompress back to FP16 when pulled into Tier 0 (HBM2).
Reference: LLM.int8() paper.

#### 3. Zero-Allocation Inference Loop
Pre-allocate ALL buffers at startup. Use arena allocators for temporary tensors.
No malloc during generation — eliminates GC pauses and fragmentation.
Crates: mimalloc or jemallocator for NUMA-aware allocation.

#### 4. Adaptive Context with "Memory Tokens"
When context exceeds capacity, don't just evict oldest KV heads.
Instead: summarize older segments into compressed "memory tokens"
that preserve key information without consuming full attention slots.
This is the "Deep Time" memory for 20-day temporal stacks.

#### 5. Domain-Specific Grammar Constraints
Extend grammar-constrained sampling to include maritime rules:
- Valid latitude/longitude ranges (Great Lakes bounds)
- Valid depth ranges (0-1000ft for Erie)
- Valid tool call schemas
- Prevents the model from hallucinating impossible coordinates

#### 6. Early-Exit Mechanism
For simple queries, allow the model to exit before reaching final layers.
If confidence is high after layer N, skip remaining layers.
Saves compute on easy tasks (health checks, simple file reads).

#### 7. Per-Token Telemetry
Track: token generation latency, memory usage per tier,
GPU utilization, expert activation patterns, KV cache hit rates.
Enables real-time optimization and bottleneck identification.

### Rejected/Inapplicable Suggestions:
- "Drop QUIC" — QUIC is for cross-NODE (cesarops2/3), not intra-GPU. PCIe handles local.
- "Use CUDA backend" — locks us to NVIDIA. wgpu is hardware-agnostic (the whole point).
- "Continuous batching" — single-user system, not a serving farm.
- "Federated inference across GPUs" — we already do tensor parallel, this adds complexity for no gain.

## Gemini Refinements (May 2026)

### WGSL FlashAttention Strategy
- WGSL lacks CUDA shuffle intrinsics — use Workgroup Shared Memory tiling instead
- P100 has massive register file — afford larger tile sizes than consumer cards
- Minimize global memory round-trips by keeping Q,K,V tiles in shared memory
- This is the highest ROI task for the engine

### Tiered KV Compression Pipeline
- Active Tier (GPU HBM2): FP16 full precision
- Warm Tier (DDR4): INT8 quantized via bytemuck zero-copy casting
- Cold Tier (RAID): Memory Tokens (cross-attention summaries)

### Arena Allocators
- Use `bumpalo` or `typed-arena` crates for the inference loop
- Pre-allocate massive chunk of NUMA-pinned memory at startup
- During forward pass: just move an offset pointer (zero malloc)
- Eliminates jitter from system allocator during generation

### Grammar Constraints — Logit Processor
- Mask invalid tokens directly in the sampler
- Latitude: [-90, 90], Longitude: [-180, 180]
- Depth: [0, 1000] for Erie
- Bake into sampling.rs as a pre-softmax logit mask
- Trivial compute cost, massive reliability gain

### Memory Tokens — Cross-Attention Approach (CHOSEN)
Why cross-attention over compressive transformer:
- SAR needs selective access to historical data (day 3 thermal, day 12 current)
- Fixed-rate compression loses detail uniformly — bad for wreck hunting
- Cross-attention lets model "peek" at specific historical points on demand

Implementation:
1. When KV heads evicted from Tier 0 → run lightweight compression pass
2. Produce fixed-size "memory tokens" (vectors encoding key information)
3. Store in separate "memory bank" (DDR4 or RAID)
4. During attention: model cross-attends to memory bank alongside active context
5. Cost: one extra attention head per layer (small overhead)
6. Benefit: effectively infinite context with selective recall
