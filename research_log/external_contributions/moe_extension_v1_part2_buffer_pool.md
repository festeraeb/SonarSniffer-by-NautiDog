# MoE Extension v1 — Part 2: Lock-Free Buffer Pool

Source: dropped in by operator from a friend, 2026-05-16. Companion to
`moe_extension_v1.md` (the shaders + dispatcher).
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

This part adds a runtime allocation layer so the MoE forward pass doesn't
hit `device.create_buffer` on every layer per token. The design is:

- One `MoeScratchChunk` per inference slot, sized for max_batch × num_experts × intermediate_dim.
- A pool of N pre-allocated chunks held in a `crossbeam_queue::ArrayQueue` (lock-free).
- An `acquire()` returns an RAII `MoePoolGuard` that hands a chunk back to the queue on `Drop`.
- Pool exhaustion fallback: allocate a fresh chunk on demand instead of blocking.

Why we want this once we go live with MoE:
- Today's regular forward pass already pre-allocates scratch via `ScratchBuffers`.
  MoE adds 5 more buffers per layer that would otherwise hit the wgpu allocator
  every layer per token. On Pascal Vulkan, fresh `create_buffer` calls are
  ~hundreds of microseconds. Across 28 layers × 1 token that's an extra ~10ms
  per token of pure allocator overhead.
- The pool turns those 28 allocations per token into 28 lock-free dequeues,
  which is sub-microsecond.

---

## MoeScratchChunk

```rust
pub struct MoeScratchChunk {
    pub id: usize,
    pub topk_indices_buf: Buffer,    // [max_batch * 2] u32
    pub topk_scales_buf: Buffer,     // [max_batch * 2] f32
    pub coalesced_map_buf: Buffer,   // [max_batch * num_experts_per_token] u32
    pub offsets_buf: Buffer,         // [num_experts + 1] u32
    pub intermediate_output_buf: Buffer, // [max_batch * top_k * intermediate_dim] f32
}

impl MoeScratchChunk {
    pub fn new(device: &wgpu::Device, max_batch_size: u32, num_experts: u32, intermediate_dim: u32) -> Self {
        let aligned_batch = (max_batch_size + 63) & !63;
        // ... 5 create_buffer calls, all STORAGE | COPY_SRC | COPY_DST
        // (intermediate_output_buf only needs STORAGE | COPY_SRC since it's overwritten each call)
    }
}
```

Sizes are fixed at engine init — `max_batch_size` is the upper bound on
concurrent prefill batch dimension. For decode (m=1) we waste a little
VRAM but that's cheap relative to weights.

---

## RAII Guard — automatic recycle on Drop

```rust
pub struct MoePoolGuard {
    chunk: Option<MoeScratchChunk>,
    pool: Arc<MoeBufferPoolInner>,  // contributor used `inner` field name in .new() but `pool` in struct decl — needs reconciling on integration
}

impl MoePoolGuard {
    pub fn get(&self) -> &MoeScratchChunk { self.chunk.as_ref().unwrap() }
}

impl Drop for MoePoolGuard {
    fn drop(&mut self) {
        if let Some(chunk) = self.chunk.take() {
            self.pool.queue.push(chunk);
            // Note: ArrayQueue::push returns Result<(), T> — if the queue is
            // full we silently drop. That's fine because the pool was sized
            // for steady-state load; an over-push means we got an emergency
            // alloc and the queue going back to its target depth is correct.
        }
    }
}
```

**Polish note for integration**: contribution has the field named both
`pool` (in struct) and `inner` (in builder) — must pick one and stay
consistent. `inner: Arc<MoeBufferPoolInner>` matches the `MoeBufferPool`
wrapper type below.

---

## Lock-Free Pool (crossbeam_queue::ArrayQueue)

```rust
pub struct MoeBufferPoolInner {
    pub queue: ArrayQueue<MoeScratchChunk>,
    device: Arc<wgpu::Device>,
    max_batch_size: u32,
    num_experts: u32,
    intermediate_dim: u32,
}

#[derive(Clone)]
pub struct MoeBufferPool {
    inner: Arc<MoeBufferPoolInner>,
}

impl MoeBufferPool {
    pub fn new(device, pool_depth, max_batch_size, num_experts, intermediate_dim) -> Self {
        // Pre-populates `pool_depth` chunks, falls back to ephemeral alloc on exhaustion.
    }

    pub fn acquire(&self) -> MoePoolGuard {
        match self.inner.queue.pop() {
            Some(chunk) => MoePoolGuard { chunk: Some(chunk), inner: self.inner.clone() },
            None => {
                // Emergency alloc — log a warning, this means pool_depth was
                // undersized for current concurrency.
                let chunk = MoeScratchChunk::new(&self.inner.device, ...);
                MoePoolGuard { chunk: Some(chunk), inner: self.inner.clone() }
            }
        }
    }
}
```

**Polish note**: builder uses `let _ = queue.push(chunk)` to discard the
Result — fine because we just allocated to fit.

**Cargo.toml impact**: adds `crossbeam-queue = "0.3"` dependency.
Already pulls in `crossbeam-utils` transitively which is small. No async
runtime needed — `ArrayQueue` is sync.

---

## Pooled Dispatch Path

Refactored `MoEDispatcher::execute_pooled_moe_layer` replaces the
`create_buffer`-on-every-call pattern from part 1 with `pool.acquire()`.
Three buffer descriptors that DO stay per-call (parameter uniforms) are
small (16 bytes each) and could easily be moved into push constants
during integration to drop them entirely.

Down-projection still calls `dispatch_down_projection` which itself
allocates an `output_hidden_states` buffer per call — that should also
move to the pool in v2 (track as TODO; the contributor explicitly leaves
it for the integration pass).

```rust
pub fn execute_pooled_moe_layer(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    config: &ModelConfig,
    moe_config: &MoEConfig,
    moe_pool: &MoeBufferPool,         // NEW
    weights: &MoELayerWeights,
    input_hidden_states: &wgpu::Buffer,
    batch_size: u32,
) -> wgpu::Buffer {
    let guard = moe_pool.acquire();
    let scratch = guard.get();
    // ... gate pass + CPU readback + token sort + expert MLP + down-proj ...
    // guard drops here, chunk recycles
}
```

---

## Open issues to handle during integration

1. **CPU readback hard-syncs the GPU queue every layer** (still). The
   buffer pool fixes the *allocation* cost but not the *latency* cost.
   The promised follow-up is a GPU prefix-sum scan replacement.

2. **`output_hidden_states` allocated by `dispatch_down_projection`** isn't
   pooled. Same allocator-overhead pattern as before, just one buffer
   instead of five. Move to pool in next iteration.

3. **Pool depth sizing**: contributor says "pre-populate pool_depth
   allocation blocks" but doesn't suggest a value. Sensible default:
   `pool_depth = num_concurrent_inference_slots × num_layers`. For our
   single-process v1 (1 slot × 28 layers) → pool_depth = 32.

4. **Field name inconsistency** in the `MoePoolGuard` struct (`pool` vs
   `inner`). Polish during integration.

5. **`rand::random::<usize>()` for chunk ID** introduces `rand` crate
   dependency. Replace with an `AtomicUsize::fetch_add(1, ...)` counter
   to avoid the new dep.

6. **`wgpu::util::DeviceExt`** is already in our deps for `create_buffer_init`
   (used in pipeline_init.rs), so no new dep there.

---

## Integration order

When we pick this up after multi-model loading lands:
1. Land part 1 first (shaders + basic dispatcher with per-call allocation).
2. Land part 2 (this file) as the second commit. Pool is a layered
   improvement; if part 1 isn't working part 2 doesn't help.
3. Drop CPU readback → GPU prefix-sum scan as part 3.
4. Move `output_hidden_states` into the pool as part 4.
5. Validate against Gemma-4-26B-MoE-IQ4_XS koboldcpp output.
