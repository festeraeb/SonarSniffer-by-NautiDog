// Generic f32 matmul shader
// Target: SM 6.1 (GTX 1070, P1000, P106) and any unknown hardware
// DO NOT use f16 on these cards — 1:64 ratio makes it SLOWER than f32
// Workgroup: 16x16 = 256 threads

struct Matrix {
    data: array<f32>,
};

@group(0) @binding(0) var<storage, read> matrixA: Matrix;
@group(0) @binding(1) var<storage, read> matrixB: Matrix;
@group(0) @binding(2) var<storage, read_write> matrixC: Matrix;

struct Uniforms {
    dimA: vec2<u32>,
    dimB: vec2<u32>,
};
@group(0) @binding(3) var<uniform> uniforms: Uniforms;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let row = global_id.y;
    let col = global_id.x;

    if (row >= uniforms.dimA.x || col >= uniforms.dimB.y) {
        return;
    }

    var sum: f32 = 0.0;
    for (var k: u32 = 0u; k < uniforms.dimA.y; k = k + 1u) {
        let a = matrixA.data[row * uniforms.dimA.y + k];
        let b = matrixB.data[k * uniforms.dimB.y + col];
        sum = sum + a * b;
    }

    matrixC.data[row * uniforms.dimB.y + col] = sum;
}
