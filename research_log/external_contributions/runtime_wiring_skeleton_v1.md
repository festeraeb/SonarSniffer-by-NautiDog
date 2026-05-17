# Runtime Wiring Skeleton v1 — Module Layout Reference

Source: cluster, 2026-05-17. Operator-handed scaffold showing how the
5 missing systems (MoE, cross-GPU, sliding-window attention, KV
prefix cache, UBO pool, diagnostics) connect into a unified runtime
loop.
Status: **REFERENCE — STRUCTURAL TEMPLATE, NOT FOR DROP-IN.** The
operator's framing: "I know we have matmuls and such — I wanted a
generic file for you to tweak with what we have."

## What this stash is

A wiring-and-connection skeleton. NOT an implementation. Not a code
dump. The cluster wrote a 9-file Rust scaffold that shows where each
of the queued systems plugs into a unified `Runtime::forward()` loop.

Useful for: confirming we have the right module decomposition, seeing
how the systems compose, identifying integration points we haven't
formalized yet.

NOT useful for: dropping into the engine. Our existing implementations
of these subsystems are far more sophisticated than the scaffold
versions, and we already have most of the connection points wired.

## Cluster's module layout vs ours

| Scaffold module | Our equivalent | Status |
|-----------------|---------------|--------|
| `tensor.rs` | `tensor_chunker.rs` + GPU-side wgpu Tensors | ours is real, this is `Vec<f32>` |
| `moe.rs` | `src/moe.rs` (138 LOC stub, CPU f64 router) | ours has the router, no GPU dispatch |
| `attention.rs` | `attention.rs` + `attention_dispatch.rs` (full GPU split-by-head) | ours is real, this is CPU-mock |
| `kv_cache.rs` | `kv_cache.rs` + `cake_kv.rs` | ours is real, NO prefix cache yet |
| `cross_gpu.rs` | NOT IMPLEMENTED | both placeholder; cluster's is `println!` mock |
| `ubo.rs` | NOT IMPLEMENTED | both placeholder; queued in fleet prompt H |
| `diagnostic.rs` | scattered across forward_pass.rs | ours has CPU readbacks; queued for replacement in H |
| `runtime.rs` | `forward_pass.rs::execute_layer` + `transformer.rs::TransformerDecoder.forward()` | ours is real |

## Useful patterns to lift

### 1. The unified runtime forward signature

```rust
impl Runtime {
    pub fn forward(&mut self, tokens: Vec<u32>) -> Tensor {
        let prefix_hash = hash_tokens(&tokens);
        let kv = self.kv_prefix.get_or_insert(prefix_hash, || KvCache::new(1024));
        // ...
    }
}
```

This shape — prefix_hash lookup as the FIRST step in forward — is
exactly the integration point KV prefix cache should hit. Worth
adopting verbatim when wiring fleet prompt I's output.

### 2. The diagnostic gate inspect-after-each-layer pattern

```rust
self.diag.inspect("attn_out", &x);
```

Direct, simple, optional. Slots cleanly between our existing
`execute_layer` calls. Fleet prompt H's diagnostic gate should
implement this exact callsite shape.

### 3. The CrossGpuRouter abstraction over layer assignment

```rust
pub struct CrossGpuRouter {
    pub shards: Vec<GpuShard>,
}

pub struct GpuShard {
    pub device_id: usize,
    pub layer_ids: Vec<usize>,
}
```

Matches the layer-split design from fleet prompt L. The cluster's
version has the right abstraction shape — just needs to swap the
`println!` for an actual device.queue.submit + cross-device transfer.

### 4. The wiring sequence in `forward()`

Their order:
1. Hash prompt → prefix cache lookup (prefill shortcut)
2. MoE router decision (if applicable)
3. Per-layer: cross-GPU route → attention → KV update
4. Diagnostic gate samples after each layer

This sequence matches what the engine SHOULD do. Useful as a
reference order when wiring the queued systems incrementally.

## What's wrong / not for drop-in

### 1. Tensor::Vec<f32> is CPU-only

Their `Tensor { data: Vec<f32>, shape: Vec<usize> }` is the wrong
abstraction for our wgpu engine. We work with `wgpu::Buffer` handles
+ shape metadata via `tensor_chunker.rs`. Their scaffold can't actually
run anything on a GPU because every operation is CPU.

### 2. Sliding window attention is a CPU loop, not a kernel

```rust
for i in 0..seq {
    for j in start..=i {
        let score = q.data[i] * k.data[j];
        ...
    }
}
```

This is a pedagogical sketch. Real Gemma sliding window needs a WGSL
kernel that masks within `(i - j) <= window` per Q-head per token. Our
existing `attention_pc.wgsl` would need a sliding-window variant or a
push-constant flag for window size; that's a concrete kernel addition
not captured in any fleet prompt yet.

**Track:** when Gemma-4 work starts, sliding-window attention is its
own ~150 LOC WGSL extension, not a one-line addition. Fleet prompt for
it would be similar to the K_q6k_native_dispatch_unblock pattern —
shader extension + dispatch routing.

### 3. MoE router is missing the dispatch half

Their MoE module returns expert_ids and weights but doesn't dispatch
to per-expert FFN kernels. That's the actual hard part. Our existing
`src/moe.rs` has the same gap (TODO: Dispatch to GPU comments). The
cluster's `moe_extension_v1.md` and `moe_extension_v1_part2_buffer_pool.md`
stashes have the dispatch logic — that's where the real MoE work
lives.

### 4. PrefixCache stores full KvCache by value

```rust
pub map: HashMap<u64, KvCache>,
```

Storing entire KV caches per cached prefix is a memory bomb. Our
fleet prompt I addresses this correctly with KvSlice + opaque GPU
buffer handles, eviction by node not by token, generational arena
backing. Their version is the naive form to NOT ship.

### 5. SipHasher13 is deprecated

```rust
use std::hash::{Hasher, SipHasher13};
```

`SipHasher13` was deprecated in Rust 1.13 and removed from std. Use
`DefaultHasher` or `FxHash` (already in our deps). Code wouldn't
compile as-is.

## Decision

**Treat as architectural mnemonic, not implementation.** The 9-module
layout is useful for confirming we have the right decomposition. The
wiring sequence in `Runtime::forward()` is useful as a reference for
integration order. The individual modules are CPU-only sketches that
our existing wgpu implementations supersede.

## What slots into the existing fleet prompt batch

Cross-checking cluster's scaffold against H-M:

- **diagnostic.rs** → fleet prompt H (cluster's wiring template
  matches what we asked for)
- **ubo.rs** → fleet prompt H (same scope, much more detail in our
  prompt)
- **kv_prefix_cache.rs** → fleet prompt I (cluster's version is the
  naive form we asked NOT to ship)
- **cross_gpu.rs** → fleet prompt L (we asked for real implementation,
  cluster's scaffold is the placeholder)
- **moe.rs** routing → existing src/moe.rs (already has the router,
  needs the dispatch half from MoE stashes)
- **sliding window attention** → NEW gap, not in any fleet prompt yet,
  needed for any Gemma-class model

## New gap: sliding window attention prompt

Worth adding as fleet prompt N when batch H-M results come back. Scope:

```
Extend our existing attention_pc.wgsl + attention_dispatch.rs with
sliding-window support for Gemma-class models. Push-constant flag
SLIDING_WINDOW_BIT + window_size param. Mask test inside the score
computation: scores OUTSIDE (q_pos - window_size, q_pos] become -inf
before softmax. Drop into existing dispatch path with no new kernel
file - same shader, two execution modes via push-constant flag.
```

Track for next-batch dispatch.

---

Filed under research_log because the structural template is useful
for confirming module decomposition + wiring order. Individual module
contents superseded by our existing implementations and by fleet
prompts H-M.

## Verbatim source preserved in chat history. End.
