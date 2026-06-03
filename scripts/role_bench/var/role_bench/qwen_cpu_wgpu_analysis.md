# Qwen3.6-35B-A3B UD-Q4_K_XL (max ctx)

- url: http://127.0.0.1:5210
- elapsed_s: 1869.3
- usage: {"completion_tokens": 4096, "prompt_tokens": 3617, "total_tokens": 7713, "prompt_tokens_details": {"cached_tokens": 0}}

### Top 8 Bottlenecks (Ranked by Impact)

1. **PCIe Sync & CPU Readback Stalls** – `gemma4_gpu_runner.rs:4-7, 18-20, 33-34`  
   ~360 CPU↔GPU round trips/token + async readbacks for MoE top-k and vocab logits. On Pascal/Kepler, PCIe latency + queue sync dominates single-token latency.

2. **CPU Fallbacks for Attention & Geo Filter** – `backend_wgpu.rs:43-45, 54-63`  
   GPU sits idle while CPU runs attention and element-wise geo subtraction. Causes pipeline stalls and wastes Pascal's FP32 throughput.

3. **Fixed Buffer Allocation & Dispatch Granularity** – `gpu_context.rs:12-16`  
   Single `buf_a/b/c/dims` per context forces reallocation or mapping per matmul. Small dispatches (< 256 threads) underutilize Pascal's SMs.

4. **Artificial VRAM/Binding Caps** – `gpu_context.rs:45-46`  
   Hardcoded 1GB `max_storage_buffer_binding_size` and `max_buffer_size`. A 26B MoE model + KV caches will fragment or fail on real hardware.

5. **MoE Top-K Synchronization Barrier** – `gemma4_gpu_runner.rs:33-34`  
   CPU readback of 128 f32 router logits per layer blocks the compute queue. Cannot overlap with next layer's matmuls.

6. **Naive FP32 Matmul Shader** – `gpu_context.rs:54` (`matmul_f32.wgsl`)  
   Pascal lacks Tensor Cores; Kepler has a tiny register file. Untiled FP32 matmuls thrash L1/texture cache and miss vector loads.

7. **Per-Layer Pipeline/Bind Group Creation** – `pipeline_cache.rs:67`  
   Creating pipelines or bind groups per layer adds driver overhead. Pascal's Vulkan driver stalls on frequent pipeline recompilation.

8. **Buffer Alignment & Binding Limit Quirks** – `gpu_context.rs:6
