# wgpu_hal Memory-Type-Aware Scratch Pool + FFN Fusion — PARTIAL DROP

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-17. Continuation of the scratch-pool + FFN fusion
prompt teed up at end of session.
Status: **INCOMPLETE — drop was truncated mid-function. Do not
integrate against this stash until the missing portions arrive. Stub
exists so the framing isn't lost.**

## What we got before the drop cut off

### 1. The high-level framing (kept)

> "Scratch pool = memory placement problem (bandwidth + residency)
>  FFN fusion = compute + bandwidth reduction problem (MLP dominates
>  decode)"

This split is right. They're tightly coupled — FFN is the largest
single allocator of scratch in our pipeline (17 MB intermediate at
M=512, dominates everything else combined) so the pool's placement
decisions matter most for FFN.

### 2. Memory tier abstraction (full)

```rust
enum ScratchMemoryTier {
    DeviceLocalFast,     // VRAM, optimal for KV + FFN intermediates
    DeviceLocalShared,   // fallback GPU local, lower priority
    HostVisibleStaging,  // only for upload / rarely readback
}
```

Reasonable shape. Maps cleanly onto Vulkan memory type flags:
- `DeviceLocalFast` = `DEVICE_LOCAL_BIT` (no host coherency)
- `DeviceLocalShared` = `DEVICE_LOCAL_BIT` but might be a less-preferred
  heap (e.g. on integrated graphics where the only "device local"
  is the unified host pool)
- `HostVisibleStaging` = `HOST_VISIBLE_BIT | HOST_COHERENT_BIT`

The three-tier model collapses well into our existing pool design —
each `ScratchPool` instance pins to one tier, the `ScratchManager`
holds one of each as needed.

### 3. The missing piece (truncated)

Function `classify_memory_type` started but cut off at:

```rust
fn classify_memory_type(props: &vk::PhysicalDeviceMemoryProperties) -> ScratchLayout {
    // prioritize:
    [TRUNCATED]
```

We're missing:
- The full classification logic (which heap maps to which tier)
- The `ScratchLayout` type definition
- The FFN fusion section entirely
- Per-card guidance for how scratch placement differs P100 vs 1070
- Integration plan / wiring back to the existing ScratchPool skeleton

## What's actually useful in the partial

Just the framing and the tier enum. Roughly 5% of what the full drop
is going to be. Not enough to do anything with.

## Action when drop completes

When the rest arrives:
1. Replace this stash with the full version (rename to remove `_PARTIAL`)
2. Apply standard polish-note pass for compilation issues, naga
   gotchas, and the recurring naive-softmax / GQA / MHA-default traps
3. Cross-reference against the attention_scratch_pool_v1 stash since
   this drop EXTENDS that one; the polish notes from that stash
   probably apply transitively
4. The FFN fusion section, when it lands, fills the explicit gap I
   flagged in the fusion_rewrite_architecture stash polish-note 2

## Why this matters when complete

The two unaddressed-by-cluster items in our queue right now are:
- wgpu_hal-friendly pool variant (cluster offered at end of attention
  scratch pool drop)
- Fused FFN kernel design (gap in fusion rewrite drop)

This drop is meant to address BOTH. So when complete it closes both
remaining cluster-input gaps and we'd have full coverage of the
queued integration work.

Until it completes, the integration plan stays where it was:
attention_scratch_pool_v1 is the integration-ready pool design,
fusion_rewrite_architecture has the FFN gap as known.

## Verbatim source — what arrived (incomplete)

```rust
enum ScratchMemoryTier {
    DeviceLocalFast,     // VRAM, optimal for KV + FFN intermediates
    DeviceLocalShared,   // fallback GPU local, lower priority
    HostVisibleStaging,  // only for upload / rarely readback
}

fn classify_memory_type(props: &vk::PhysicalDeviceMemoryProperties) -> ScratchLayout {
    // prioritize:
```

End of received content. Drop is truncated; the cluster's coding agent
either timed out / hit free-tier limit / lost connection mid-stream.
