---
inclusion: manual
---

# Warp-Grid Hardware Hacks — MoE Context Injection

This document contains hardware-specific knowledge that MUST be injected into the
MoE's context when generating warp-grid code. These are not generic Rust patterns —
they are specific to the CESAROPS T440 cluster hardware.

## 1. Pascal P100 FP16 Half2 Arithmetic (GP100 — SM 6.0)

The Tesla P100 is the ONLY Pascal chip with 2:1 FP16-to-FP32 throughput.
On the GTX 1070/P1000/P106 (SM 6.1), FP16 is emulated at 1:64 ratio — SLOWER than FP32.

### Rules for Code Generation:
- P100 shaders MUST use `f16vec2` (Half2) packed operations
- Peak throughput (~21 TFLOPS) is achieved ONLY by pairing FP16 ops into vec2
- Single `f16` operations do NOT get the 2:1 speedup — must be paired
- SPIR-V requires: `OpCapability Float16`, `SPV_KHR_16bit_storage`, `StorageBuffer16BitAccess`
- WGSL requires: `enable f16;` extension (wgpu 29 / naga 29 support)
- GLSL requires: `GL_EXT_shader_explicit_arithmetic_types_float16`

### WRONG (no speedup — single f16):
```wgsl
var val: f16 = input[id];
val = val * 1.01h;
```

### RIGHT (2:1 speedup — paired vec2<f16>):
```wgsl
var val: vec2<f16> = input_packed[id];
let factor = vec2<f16>(1.01h, 1.01h);
input_packed[id] = fma(val, factor, val);
```

### SM 6.1 (1070/P1000/P106) — DO NOT USE FP16:
These cards have a 1:64 FP16 ratio. Using f16 shaders on these cards is SLOWER
than f32. Always fall back to the generic f32 shader path for SM 6.1 targets.

## 2. Xeon Silver 4110 AVX-512 Frequency Throttling

CRITICAL: AVX-512 FMA instructions cause frequency drops on Xeon Silver.

### The Problem:
- Xeon Silver 4110 base clock: 2.1 GHz
- With AVX-512 FMA active on ≥9 cores: clock drops to ~1.4 GHz on ALL cores
- This affects EVERY thread on the chip, not just the ones running AVX-512
- The voltage transition takes ~20µs — during which ALL execution stalls
- This means: if you run AVX-512 on the curvelet threads, the GPU-feeding
  threads on the same socket ALSO slow down

### Rules for Code Generation:
- Use AVX-512 SPARINGLY — only for the actual math-heavy inner loops
- Prefer short bursts of AVX-512 over sustained execution
- NEVER run AVX-512 on all 16 cores simultaneously (guaranteed 1.4GHz drop)
- For the drift correction phase: use AVX-512 on 4-8 cores max, leave the
  rest at full clock for GPU staging and network I/O
- Consider `pulp` crate for safe SIMD that auto-selects instruction width
- If possible, isolate AVX-512 work to one socket and keep the other socket
  at full clock for latency-sensitive tasks (GPU feeding, QUIC transport)

### WRONG (all cores doing AVX-512 — 1.4GHz everywhere):
```rust
use rayon::prelude::*;
data.par_chunks_mut(16).for_each(|chunk| {
    unsafe { avx512_fma(chunk); } // ALL 32 threads doing AVX-512
});
```

### RIGHT (limited cores, burst pattern):
```rust
// Create a dedicated thread pool with only 8 threads for AVX-512 work
let avx_pool = rayon::ThreadPoolBuilder::new()
    .num_threads(8)  // Only 8 cores — stays above 1.4GHz threshold
    .build().unwrap();

avx_pool.install(|| {
    data.par_chunks_mut(16).for_each(|chunk| {
        unsafe { avx512_fma(chunk); }
    });
});
// Other 24 threads remain at 2.1GHz for GPU staging + network I/O
```

## 3. NUMA Topology — T440 Dual Socket

The T440 has two Xeon Silver 4110 processors connected via UPI (Ultra Path Interconnect).
Crossing the UPI adds ~100ns latency and loses ~40% memory bandwidth.

### Physical Layout:
```
Socket 0 (Cores 0-7, HT 16-23):
  └── DDR4 Channels 0-2 (64 GB/s aggregate)
  └── PCIe 3.0 x16 → P100 #0 (15.75 GB/s each direction)

Socket 1 (Cores 8-15, HT 24-31):
  └── DDR4 Channels 3-5 (64 GB/s aggregate)
  └── PCIe 3.0 x16 → P100 #1 (15.75 GB/s each direction)

UPI Link: ~38.4 GB/s (PENALTY PATH — avoid for GPU staging)
```

### Rules for Code Generation:
- Threads feeding P100 #0 MUST be pinned to Cores 0-7 (Socket 0)
- Threads feeding P100 #1 MUST be pinned to Cores 8-15 (Socket 1)
- Staging buffers for P100 #0 MUST be allocated on Socket 0's NUMA node
- Staging buffers for P100 #1 MUST be allocated on Socket 1's NUMA node
- AVX-512 drift correction: prefer Socket 0 (or whichever has more free RAM)
- QUIC network I/O threads: pin to whichever socket has the NIC's PCIe slot
- Use `hwloc2` crate to discover topology at runtime (don't hardcode core IDs)

### Detecting NUMA Status:
```bash
numactl --hardware
# Expected output (Node Interleaving DISABLED):
# available: 2 nodes (0-1)
# node 0 cpus: 0 1 2 3 4 5 6 7 16 17 18 19 20 21 22 23
# node 0 size: 47168 MB
# node 1 cpus: 8 9 10 11 12 13 14 15 24 25 26 27 28 29 30 31
# node 1 size: 47168 MB
```

If it shows 1 node with all 94GB → Node Interleaving is ON in BIOS → MUST disable.

### PCIe Bus Discovery (hwloc2):
```rust
// Find which socket owns a GPU's PCIe slot
use hwloc2::{Topology, ObjectType};

let topo = Topology::new().unwrap();
// Traverse PCI objects to find P100 by vendor:device (10DE:15F8)
// Then walk up to find the parent Package (socket)
// P100 #0 on bus 0000:3b:00.0 → Socket 0
// P100 #1 on bus 0000:86:00.0 → Socket 1
```

## 4. nauticuvs Signal Processing API

The `nauticuvs` crate provides curvelet transforms for aeromagnetic/thermal analysis.
warp-grid should use these for initial tile processing BEFORE handing to GPU shaders.

### Key Functions:
- `curvelet_forward(input: &Array2<f32>, num_scales: usize)` → CurveletCoeffs
- `curvelet_inverse(coeffs: &CurveletCoeffs)` → Array2<f32>
- `CurveletConfig` — scale count, angular resolution, wrapping mode

### Integration Pattern:
1. CPU (Xeon AVX-512): Run `curvelet_forward()` on raw GeoTIFF tile
2. Extract frequency-domain features (energy per scale/angle)
3. GPU (P100 Half2): Run dipole detection shader on the curvelet coefficients
4. CPU: Run `curvelet_inverse()` to reconstruct filtered tile if needed

### Why This Order:
- Curvelet forward has sequential data dependencies → CPU (AVX-512)
- Dipole pixel scan is embarrassingly parallel → GPU (P100 Half2 shader)
- This matches the existing `fdct_kernels.rs` routing: sequential → Xeon, parallel → P100

## 5. Workgroup Size Optimization

### P100 (56 SMs, 64 CUDA cores per SM = 3584 total):
- Optimal workgroup: 32×32 = 1024 threads (fills SM occupancy)
- Alternative: 64×1 for 1D data (matches warp size × 2)
- The existing `nauticus_scanner.wgsl` uses 32×32 — keep this pattern

### 1070 (15 SMs, 128 CUDA cores per SM = 1920 total):
- Optimal workgroup: 16×16 = 256 threads
- The existing `dipole_shader.wgsl` uses 16×16 — correct for SM 6.1

### Turing (future, SM 7.5):
- Optimal workgroup: 32×32 with subgroup operations
- Can use `subgroupBallot` for warp-level reduction

## 6. Memory Bandwidth Reality

| Path | Bandwidth | Use For |
|------|-----------|---------|
| P100 HBM2 (internal) | 732 GB/s | Shader compute, KV cache |
| DDR4 Socket-local | 64 GB/s | Staging buffers, CPU compute |
| DDR4 Cross-socket (UPI) | ~38 GB/s | AVOID — penalty path |
| PCIe 3.0 x16 (host→GPU) | 15.75 GB/s | Tile upload, result readback |
| 1Gbps Ethernet (LAN) | 0.125 GB/s | Remote peer offload |
| Tailscale (WAN) | ~0.05-0.1 GB/s | Friend's P100s, WebGPU peers |

### Implication for Scheduler:
- Local P100 dispatch: limited by PCIe upload (15.75 GB/s) not GPU compute
- Remote peer offload: only worth it for tasks > 100ms compute time
  (otherwise network latency dominates)
- DDR3 nodes (cesarops2/3): limited to ~25 GB/s — preprocessor role only
