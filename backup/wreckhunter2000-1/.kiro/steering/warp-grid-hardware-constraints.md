# Warp-Grid Hardware Constraints — MoE Grounding Context

This file MUST be injected into the MoE's context (via nautivecs) before any
warp-grid implementation task. It contains hardware-specific constraints that
prevent the MoE from generating "legal but slow" code.

## 1. Pascal GP100 (P100) — SPIR-V Validation Manifesto

### Required Capabilities for FP16 2:1 Throughput

Every P100-bound shader MUST declare these SPIR-V capabilities. If ANY is missing,
the shader falls back to slow f32 emulation or fails to link:

- `Shader` — base capability
- `Float16` — native 16-bit math
- `StorageBuffer16BitAccess` — read/write f16 directly from HBM2 without upcasting
- `StorageUniformBufferBlock16` — same for uniform blocks

### Required GLSL/WGSL Header (P100 targets only)

```glsl
#version 450
#extension GL_EXT_shader_explicit_arithmetic_types_float16 : require
#extension GL_EXT_shader_16bit_storage : require

layout(local_size_x = 64) in;  // 32 threads × 2 for Pascal warp
```

For WGSL (wgpu 29.x):
```wgsl
enable f16;

@compute @workgroup_size(64)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    // Use vec2<f16> for HFMA2 exploitation
}
```

### P100 Register Pressure Rule

Pascal GP100 has a limited register file per SM (65536 registers / max 2048 threads = 32 per thread at full occupancy).

**RULE**: If a shader uses > 32 registers per thread, SM occupancy drops.
**GUARD**: If the MoE generates a complex shader with many local variables,
shard the kernel into two passes. This keeps HBM2 bandwidth saturated.

**Heuristic**: Count local variables + temporaries. If > 20 named f32/f16 values
in a single function, split into two dispatches with an intermediate buffer.

### SM 6.1 (1070/P1000/P106) — DO NOT USE FP16

These cards have a 1:64 FP16-to-FP32 ratio. FP16 is EMULATED and SLOWER than FP32.
Always use the generic f32 shader path for SM 6.1 targets.

## 2. Xeon Silver 4110 — AVX-512 Throttling

### The Frequency Penalty

Xeon Silver 4110 has THREE frequency tiers:
- **Non-AVX**: 2.1 GHz base, 3.0 GHz turbo (all 16 cores)
- **AVX-512 light** (1-8 cores): ~1.8 GHz
- **AVX-512 heavy** (9+ cores): drops to **1.4 GHz** across ALL cores

**CRITICAL**: If 9 or more cores execute AVX-512 FMA instructions simultaneously,
the ENTIRE chip (both sockets) drops to minimum frequency. This affects ALL threads,
even those running scalar code.

### Mitigation Rules

1. **Limit AVX-512 to 8 cores max** — use `rayon::ThreadPoolBuilder::new().num_threads(8)`
   for AVX-512 workloads
2. **Use AVX-512 sparingly** — only for the hot inner loop (curvelet windowing, FMA)
3. **Avoid sustained AVX-512** — the voltage transition halt is ~20µs. Interleave
   AVX-512 bursts with scalar work to let the chip recover frequency
4. **Never use AVX-512 during drift correction** — the drift correction phase is
   latency-sensitive. Use AVX2 (256-bit) instead, which doesn't trigger the penalty
5. **Profile with `perf stat`** — watch for `cpu-clock` drops during AVX-512 sections

### Recommended Pattern

```rust
// GOOD: Limit AVX-512 to 8 threads, use for curvelet windowing only
let avx512_pool = rayon::ThreadPoolBuilder::new()
    .num_threads(8)  // Stay below the 9-core penalty threshold
    .build()
    .unwrap();

avx512_pool.install(|| {
    output.par_chunks_mut(16)  // 16 × f32 = 512 bits = one AVX-512 register
        .zip(input.par_chunks(16))
        .for_each(|(out, inp)| {
            // AVX-512 FMA here — limited to 8 cores
        });
});

// Drift correction uses AVX2 (no frequency penalty)
// std::arch::x86_64::_mm256_fmadd_ps (256-bit, no throttle)
```

## 3. NUMA Topology — T440 Dual Socket

### Physical Layout

```
Socket 0 (Cores 0-7, HT 16-23):
  └── DDR4 Channels 0-2 (3 × 21.3 GB/s = 64 GB/s)
  └── PCIe 3.0 x16 → P100 #0

Socket 1 (Cores 8-15, HT 24-31):
  └── DDR4 Channels 3-5 (3 × 21.3 GB/s = 64 GB/s)
  └── PCIe 3.0 x16 → P100 #1

UPI Interconnect: ~38.4 GB/s (penalty path — 2× latency vs local)
```

### Rules for Code Generation

- Threads feeding P100 #0 → pin to Cores 0-7 (Socket 0)
- Threads feeding P100 #1 → pin to Cores 8-15 (Socket 1)
- Memory for P100 #0 staging → allocate on NUMA Node 0
- Memory for P100 #1 staging → allocate on NUMA Node 1
- NEVER let a Socket 0 thread feed P100 #1 (UPI penalty: ~40% bandwidth loss)
- AVX-512 threads → pin to Socket 0 (leave Socket 1 for P100 #1 feeding)

### Verification Command

```bash
numactl --hardware
# Expected output:
# available: 2 nodes (0-1)
# node 0 cpus: 0 1 2 3 4 5 6 7 16 17 18 19 20 21 22 23
# node 1 cpus: 8 9 10 11 12 13 14 15 24 25 26 27 28 29 30 31
# node 0 size: ~47000 MB
# node 1 size: ~47000 MB
```

If this shows 1 node with all 94GB → BIOS "Node Interleaving" is ON → DISABLE IT.

## 4. nautivecs Integration Bridge

### NautiBuffer → wgpu::Buffer

When converting nautivecs output to GPU-ready buffers:

```rust
use wgpu::util::DeviceExt;

// Convert nauticuvs curvelet output to GPU staging buffer
let gpu_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
    label: Some("Curvelet → P100 Staging"),
    contents: bytemuck::cast_slice(&curvelet_output),
    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
});
```

### Curvelet → P100 → Xeon Pipeline

1. **nautivecs** `curvelet_forward()` on Xeon (FP64, sequential) → produces frequency-domain coefficients
2. **P100** runs dipole detection shader on coefficients (f32 parallel, or f16 on P100 for 2:1)
3. **Xeon** runs `curvelet_inverse()` on detected anomalies (FP64, sequential) → final aeromagnetic map

The P100 NEVER does the curvelet math (WGSL doesn't support f64). It only does the
embarrassingly parallel pixel sweep BETWEEN the forward and inverse transforms.

## 5. Shader Validation Pre-Check (K-line for Tasks)

Before dispatching ANY shader to the P100s, the Kernel Forge MUST validate:

```rust
fn validate_p100_shader(spv_bytes: &[u8]) -> Result<(), ShaderRejectReason> {
    // 1. Check for required capabilities
    let has_float16 = spv_bytes.windows(4).any(|w| /* OpCapability Float16 */);
    let has_storage_16 = spv_bytes.windows(4).any(|w| /* StorageBuffer16BitAccess */);

    if !has_float16 || !has_storage_16 {
        return Err(ShaderRejectReason::MissingPascalCapabilities);
    }

    // 2. Estimate register pressure (heuristic: count OpVariable declarations)
    let var_count = count_op_variables(spv_bytes);
    if var_count > 32 {
        return Err(ShaderRejectReason::RegisterPressureTooHigh(var_count));
    }

    Ok(())
}
```

If validation fails: K-line the TASK (not the peer), log the reason, and request
a "Pascal-Specialized" retry from the MoE with this steering file injected.
