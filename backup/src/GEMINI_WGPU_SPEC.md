# SPEC FOR GEMINI: wgpu GPU Dispatch for cesarops-inference

## THE BIG PICTURE

The cesarops cluster has multiple GPUs. The architecture is:

- **Bootstrap Brain** (always-on, CPU): Qwen2.5-1.5B on cesarops-inference, port 5002. This is the recovery path. It serves the web UI and has tools to load/unload models on GPUs.
- **Worker GPUs** (P100s, future M10s, 1080): Load larger models for think/polish/code roles. These are managed by the bootstrap brain.

Your job: make cesarops-inference able to dispatch matmul to a GPU via wgpu, so when a model IS loaded on a GPU, inference runs fast. The bootstrap brain stays on CPU. Worker instances get the GPU backend.

## WHAT YOU ARE BUILDING

Wire up the existing wgpu infrastructure so that `backend_wgpu.rs` actually dispatches matmul operations to the P100 GPUs via Vulkan/wgpu instead of falling back to CPU.

## CONTEXT — WHAT ALREADY EXISTS (DO NOT REWRITE THESE)

All of these files exist and compile. You are filling in the GPU dispatch glue.

### Files you MUST NOT modify:
- `src/backend_trait.rs` — the `CesarOpsBackend` trait (interface is locked)
- `src/transformer.rs` — the forward pass (calls matmul, don't touch)
- `src/bridge.rs` — dequantization (working correctly)
- `src/server.rs` — HTTP API (working)
- `shaders/matmul_half2.wgsl` — the compute shader (written, correct)
- `shaders/geo_filter.wgsl` — detection shader (written)
- `src/wgpu_dynamic_binder.rs` — bind group generation (written)

### Files you ARE modifying:
- `src/backend_wgpu.rs` — replace CPU fallback with real GPU dispatch
- `src/wgpu_uniform.rs` — may need to create/update the MatrixDimensions struct

### File you ARE CREATING:
- `src/gpu_context.rs` — the wgpu device/queue/pipeline initialization

## HARDWARE

- 2x Tesla P100-PCIE-16GB (Pascal, SM 6.0, Vulkan 1.2)
- Future: 3x Tesla M10 (Maxwell, SM 5.2, Vulkan 1.1)
- NO CUDA. All compute goes through wgpu/Vulkan.
- P100 supports f16 at 2:1 throughput vs f32. The shader uses `vec2<f16>`.

## DEPENDENCIES ALREADY IN Cargo.toml

```toml
wgpu = "24"
half = "2"
bytemuck = { version = "1", features = ["derive"] }
```

## WHAT gpu_context.rs MUST DO

```rust
// src/gpu_context.rs
pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub matmul_pipeline: wgpu::ComputePipeline,
    // Pre-allocated buffers for the largest matmul (lm_head: 1536 × 151936)
    pub buf_a: wgpu::Buffer,
    pub buf_b_t: wgpu::Buffer,
    pub buf_c: wgpu::Buffer,
    pub buf_dims: wgpu::Buffer,
}

impl GpuContext {
    pub async fn init() -> Result<Self, String> {
        // 1. Request adapter (prefer Vulkan, high-performance)
        // 2. Request device with f16 feature enabled (required for the shader)
        //    - device.features() must include wgpu::Features::SHADER_F16
        // 3. Compile matmul_half2.wgsl into a ComputePipeline
        // 4. Pre-allocate buffers sized for the largest expected matmul
        //    - lm_head: A[1, 1536] × B_T[151936, 1536] → C[1, 151936]
        //    - buf_a: 1536 * 2 bytes (f16) = 3KB
        //    - buf_b_t: 151936 * 1536 * 2 bytes (f16) = ~446MB
        //    - buf_c: 151936 * 4 bytes (f32) = ~580KB
        //    - buf_dims: 16 bytes (MatrixDimensions uniform)
        // 5. Return the context
    }

    pub fn matmul_gpu(&self, a_f32: &[f32], b_t_f32: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
        // 1. Convert a_f32 and b_t_f32 to f16 (use half crate)
        // 2. Write f16 data to buf_a and buf_b_t via queue.write_buffer()
        // 3. Write MatrixDimensions { m, k, n, pad: 0 } to buf_dims
        // 4. Create bind group using wgpu_dynamic_binder::generate_dynamic_bind_group()
        // 5. Create command encoder, begin compute pass
        // 6. Set pipeline, set bind group
        // 7. Dispatch workgroups: ceil(n/16) × ceil(m/16) × 1
        // 8. Copy buf_c to a staging buffer (MAP_READ)
        // 9. Submit, poll device, map staging buffer
        // 10. Read f32 results back from staging buffer
        // 11. Return Vec<f32>
    }
}
```

## WHAT backend_wgpu.rs BECOMES

```rust
use crate::backend_trait::CesarOpsBackend;
use crate::gpu_context::GpuContext;
use std::sync::Arc;

pub struct WgpuPascalBackend {
    gpu: Arc<GpuContext>,
}

impl WgpuPascalBackend {
    pub async fn new() -> Self {
        let gpu = GpuContext::init().await.expect("Failed to init wgpu");
        Self { gpu: Arc::new(gpu) }
    }
}

impl CesarOpsBackend for WgpuPascalBackend {
    fn compute_matmul(&self, a: &[f32], b_t: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
        self.gpu.matmul_gpu(a, b_t, m, k, n)
    }
    // ... other methods can stay as CPU fallback for now
}
```

## MatrixDimensions STRUCT (src/wgpu_uniform.rs)

```rust
#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MatrixDimensions {
    pub m: u32,
    pub k: u32,
    pub n: u32,
    pub pad: u32,
}
```

## CRITICAL NOTES

1. The shader uses `enable f16;` — you MUST request `wgpu::Features::SHADER_F16` when creating the device. If the adapter doesn't support it, fall back to a f32 shader (or panic with a clear message).

2. The shader expects inputs as `array<vec2<f16>>` — that means the f16 data must be packed as pairs. If K is odd, pad to even.

3. Buffer sizes: wgpu requires buffer sizes to be multiples of 4 bytes. Pad accordingly.

4. The P100 has 16GB VRAM. The full model weights (~1.4GB for Q6_K 1.5B) should be uploaded once at load time, not per-token. But for the FIRST version, per-token upload is fine — optimize later.

5. `queue.write_buffer()` is synchronous from the CPU side. For the first version this is fine. Later we can use staging buffers with async mapping.

6. Workgroup dispatch: the shader uses `@workgroup_size(16, 16, 1)`. Dispatch `ceil(n/16), ceil(m/16), 1` workgroups.

7. DO NOT use `pollster::block_on` inside an async context. The server is tokio-based. Use `device.poll(wgpu::Maintain::Wait)` for synchronous GPU completion.

## TESTING

After building, test with:
```bash
curl -s -X POST http://127.0.0.1:5002/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{"prompt":"Hello","max_length":5,"temperature":0.3}'
```

If GPU dispatch works, you should see ~10-50x speedup over the current ~10 sec/token CPU path.

## WHAT NOT TO DO

- Do NOT use CUDA, cuDNN, or any NVIDIA-specific APIs
- Do NOT rewrite transformer.rs
- Do NOT change the CesarOpsBackend trait signature
- Do NOT add Python dependencies
- Do NOT use `pollster` (we're in tokio)
- Do NOT reference FlashAttention (doesn't exist in this codebase)
- Do NOT use f64 anywhere in the compute path

## REPO LOCATION

`/codebase/repos/wreckhunter2000-1/cesarops-inference/`

Build with:
```bash
source ~/.cargo/env
cd /codebase/repos/wreckhunter2000-1/cesarops-inference
cargo build --release
```


## MULTI-INSTANCE DESIGN

The inference binary must support running multiple instances:

```bash
# Bootstrap brain (always on, CPU only, port 5002)
cesarops-inference --model qwen2.5-1.5b-q6k.gguf --port 5002 --backend cpu

# Worker on P100 GPU 0 (loaded by bootstrap brain when needed)
cesarops-inference --model qwen3-8b-q4k.gguf --port 5010 --backend wgpu --gpu 0

# Worker on P100 GPU 1
cesarops-inference --model phi3-mini-q6k.gguf --port 5011 --backend wgpu --gpu 1
```

Add CLI flags:
- `--backend cpu|wgpu` — which backend to use (default: cpu)
- `--gpu N` — which GPU index to target (for wgpu adapter selection)

The `GpuContext::init()` should accept a GPU index and use it to select the correct adapter from `wgpu::Instance::enumerate_adapters()`.

## WEIGHT UPLOAD STRATEGY

For the first version:
1. At model load time, dequantize ALL weight tensors to f16
2. Upload them to GPU buffers (one buffer per tensor, or one big buffer with offsets)
3. During inference, the forward pass just references the pre-uploaded buffers

This means ~1.4GB VRAM for the 1.5B Q6_K model (dequantized to f16). The P100 has 16GB so this is fine. Larger models (8B+) will need ~8-10GB which still fits.

## DELIVERABLES

1. `src/gpu_context.rs` — device init, pipeline creation, matmul dispatch
2. `src/wgpu_uniform.rs` — MatrixDimensions struct (if not already there)
3. Updated `src/backend_wgpu.rs` — real GPU dispatch instead of CPU fallback
4. CLI flag handling in `src/main.rs` for `--backend` and `--gpu`

That's it. Don't touch the agent, the router, the detection pipeline, or the web UI.
