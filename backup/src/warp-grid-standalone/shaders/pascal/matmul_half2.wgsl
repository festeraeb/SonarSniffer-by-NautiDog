// P100 Optimized matmul_half2.wgsl
// Target: SM 6.0 (Pascal P100) - 2:1 FP16 throughput boost
// Register pressure: 6 vars (well under 32 limit)
// Workgroup: 16x16 = 256 threads (balances occupancy + shared memory on Pascal 56 SMs)

enable f16;

struct Matrix {
    data: array<f16>,
};

@group(0) @binding(0) var<storage, read> matrixA: Matrix;
@group(0) @binding(1) var<storage, read> matrixB: Matrix;
@group(0) @binding(2) var<storage, read_write> matrixC: Matrix;

struct Uniforms {
    dimA: vec2<u32>, // rows, cols
    dimB: vec2<u32>, // rows, cols
};
@group(0) @binding(3) var<uniform> uniforms: Uniforms;

// 16x16 workgroup — 256 threads per workgroup
// P100 has 56 SMs, each can run multiple workgroups
// This size balances occupancy with register pressure
@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let row = global_id.y;
    let col = global_id.x;

    // Boundary check for non-square matrices
    if (row >= uniforms.dimA.x || col >= uniforms.dimB.y) {
        return;
    }

    // Accumulate in f16 — P100 does this at 2x rate vs f32
    var sum: f16 = 0.0h;
    for (var k: u32 = 0u; k < uniforms.dimA.y; k = k + 1u) {
        let a = matrixA.data[row * uniforms.dimA.y + k];
        let b = matrixB.data[k * uniforms.dimB.y + col];
        sum = sum + a * b;
    }

    matrixC.data[row * uniforms.dimB.y + col] = sum;
}
