# wgpu → wgpu_hal::vulkan Submit-Path Port — Scoping Response

Source: dropped in by operator from a friend's wgpu/Vulkan/SPIR-V agent
cluster, 2026-05-16. Response to Prompt 6 from our outbound research
asks.
Status: **REFERENCE — APPROVED ARCHITECTURE, awaiting implementation
slot after multi-model + benchmarking land.**

## Verdict

**Proceed with the port.** Architecture boundary is right:
- KEEP in safe wgpu: Device, Buffer, BindGroup, BindGroupLayout,
  ComputePipeline, ShaderModule, pipeline cache, naga compilation,
  resource lifetime
- REPLACE: command encoder recording, queue submission, synchronization

Their reasoning matches the bottleneck list exactly: WebGPU's intentional
synchronization-hiding is what forces our ~500 submits per token. Vulkan
pre-recorded command buffers + manual `vkCmdPipelineBarrier` collapses
that to **1 submit per token**.

## Timing estimate (their breakdown, theirs not ours)

| Task | Days |
|------|------|
| HAL/raw Vulkan extraction | 2 |
| Command pool/buffer system | 2 |
| Dispatch graph recording | 2 |
| Barrier correctness | 2-3 |
| Runtime flag + fallback | 1 |
| Validation/debugging | 2-3 |
| Pascal stability testing | 2-4 |

**~2 weeks prototype, ~3-4 weeks production-quality.**

## What they confirmed about our suspicions

- "~17 submits/layer × 28 layers ≈ 476-500 submits/token is catastrophic
  for CPU overhead on Vulkan."
- "Pascal is especially sensitive because queue submit cost is
  relatively high compared to newer architectures."
- The P100 storage-buffer hazard we work around by splitting submits is
  exactly a missing-availability/visibility synchronization barrier —
  i.e., we're paying the submit cost to get the implicit barrier wgpu
  inserts at submission boundaries.
- Expected gain: **10-30% tokens/sec on small-batch autoregressive
  inference**, plus dramatically lower CPU utilization. (For our 2.2 t/s
  -> reference 60 t/s gap, this is a piece of the puzzle, not the whole
  thing — multi-model batching + packed Q6_K matvec are bigger.)

## The canonical compute->compute SSBO barrier

This is the missing piece in our current code path. Drop-in once we
have the unsafe layer:

```c
VkBufferMemoryBarrier barrier = {
    .sType = VK_STRUCTURE_TYPE_BUFFER_MEMORY_BARRIER,
    .srcAccessMask = VK_ACCESS_SHADER_WRITE_BIT,
    .dstAccessMask = VK_ACCESS_SHADER_READ_BIT,
    .srcQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
    .dstQueueFamilyIndex = VK_QUEUE_FAMILY_IGNORED,
    .buffer = buffer,
    .offset = 0,
    .size = VK_WHOLE_SIZE,
};

vkCmdPipelineBarrier(
    cmd,
    VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
    VK_PIPELINE_STAGE_COMPUTE_SHADER_BIT,
    0, 0, NULL, 1, &barrier, 0, NULL
);
```

Required between (their list, matches our forward_pass.rs structure):

| Producer | Consumer | Barrier? |
|----------|----------|----------|
| q_proj | rope | yes |
| rope | attention QK | yes |
| kv_cache_write | attention | yes |
| attention output | o_proj | yes |
| gate/up | swiglu | yes |
| swiglu | down_proj | yes |

## Their "unrealistic" warnings — pin these so we don't regress

DO for v1:
- Vulkan-only (no DX/Metal compat)
- single queue
- primary command buffers only
- conservative barriers (over-barrier rather than miss one)
- no timeline semaphores yet
- static graph recording (re-record if dims change)

DO NOT for v1:
- portable HAL backend abstraction
- dynamic graph mutation
- multi-queue
- barrier optimization passes
- timeline semaphore upgrade

## Recommended runtime structure (verbatim)

```
engine/
  backend/
    wgpu_safe.rs       <- existing path, untouched
    vulkan_fastpath.rs <- new
```

Runtime gate:
```rust
if opts.native_vk_fastpath {
    run_vk_fastpath(...)
} else {
    run_wgpu(...)
}
```

This is the rollback path we asked about — a runtime flag, both paths
in the same binary. Validation layers stay enabled in dev, disabled
in prod inference builds.

## Risk register (verbatim)

| Risk | Severity | Notes |
|------|----------|-------|
| wgpu_hal API churn | High | internal APIs not stable |
| Pascal driver synchronization quirks | High | especially around SSBO hazards |
| WGPU resource tracker conflicts | High | mixed ownership model |
| Validation spam/false positives | Medium | expected with mixed abstraction |
| Queue family mismatch | Medium | usually avoidable |
| Descriptor lifetime misuse | Medium | manageable |
| Command buffer invalidation on resize/realloc | Medium | rebuild required |
| Fence deadlock bugs | Medium | careful reset discipline |
| Pipeline compatibility mismatches | Low | stable if layouts frozen |
| Push constant range mismatch | Low | easy to validate |

## Their key technical claim we need to verify

> "wgpu_hal command encoders are still constrained by abstractions
> intended for WebGPU semantics. You specifically need: manual
> vkCmdPipelineBarrier, command buffer reuse/reset, persistent
> secondary buffers, fence management, timeline semaphore possibility
> later, explicit submit batching. The HAL abstraction does not expose
> all synchronization primitives ergonomically. Therefore: use HAL
> for access, use raw Vulkan for execution."

Implication: we extract raw `ash::vk::Device`, `ash::vk::Queue`,
`ash::vk::CommandPool`, `ash::vk::CommandBuffer` from `wgpu_hal::vulkan`
and record native VkCommandBuffers. We do NOT just use HAL's encoder
type. **Bypass HAL for execution.**

This is a stronger claim than we asked for and slightly more invasive.
The benefit is removing one more abstraction layer; the cost is the
unsafe code is more fully against ash than against HAL types. Net-net:
correct call.

## Integration order (when we're ready)

This work slots AFTER multi-model loading + benchmarking land, because:
1. Multi-model gives us the registry/scheduler shape that the fast
   path will plug into.
2. Benchmarking gives us before/after numbers to verify the gain.
3. Without those two, we're optimizing blind and have no safe rollback
   target.

When we pick this up:
1. **Spec it.** Quick Plan workflow, drawing on this doc + their risk
   register. Operator signs off.
2. Add `Cargo.toml` dep on `ash` (transitive of wgpu but not exposed —
   may already be there as a dep of wgpu_hal).
3. Build the new module skeleton: `engine/backend/vulkan_fastpath.rs`
   with the HAL handle extraction.
4. Implement the per-token forward pass for ONE model architecture
   first (Qwen 1.5B — our smoke test rig).
5. Wire the runtime flag. Default OFF until both gates pass on the
   fastpath.
6. T440 P100 + cesarops2 GTX 1070 regression must both stay green
   on the safe path AND show speedup on the fastpath.
7. Production-harden over the additional 1-2 weeks (per their
   estimate).

## Open question for ourselves

The contributor mentions the gain is "10-30% tokens/sec." That's
genuine but it's not the whole 30× gap to koboldcpp. The other big
contributors per our analysis:
- Pre-dequant of weights to f32 (4× memory pressure)
- No prefill batching (M=1 per token)
- Per-head buffer allocation in attention dispatch
- Diagnostic readbacks left in production path

The Vulkan fastpath alone won't close the 30× gap. It's part of a
stack of changes. This scoping doc is correct about its specific
contribution; we shouldn't oversell it standalone.

---

Filed under research_log because the actual integration is a
multi-week effort that needs to happen *after* the multi-model and
benchmarking work, not in parallel with it.
