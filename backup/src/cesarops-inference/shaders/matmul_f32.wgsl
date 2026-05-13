// shaders/matmul_f32.wgsl
// Tiled f32 matmul with shared memory — works on ALL Vulkan GPUs.
// Uses 16x16 workgroup tiles to minimize global memory latency.

struct MatrixDimensions {
    m: u32,
    k: u32,
    n: u32,
    pad: u32,
};

@group(0) @binding(0) var<storage, read> matrix_a: array<f32>;
@group(0) @binding(1) var<storage, read> matrix_b_t: array<f32>;
@group(0) @binding(2) var<storage, read_write> matrix_c: array<f32>;
@group(0) @binding(3) var<uniform> dims: MatrixDimensions;

// Workgroup shared memory tiles — 16x16 f32 = 1KB each
var<workgroup> tile_a: array<array<f32, 16>, 16>;
var<workgroup> tile_b: array<array<f32, 16>, 16>;

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(local_invocation_id) local_id: vec3<u32>
) {
    let row = global_id.y;
    let col = global_id.x;

    var accumulator: f32 = 0.0;
    let num_tiles = (dims.k + 15u) / 16u;

    for (var t: u32 = 0u; t < num_tiles; t = t + 1u) {
        // Load tile from global memory into shared memory
        let t_k_idx = (t * 16u) + local_id.x;

        if (row < dims.m && t_k_idx < dims.k) {
            tile_a[local_id.y][local_id.x] = matrix_a[(row * dims.k) + t_k_idx];
        } else {
            tile_a[local_id.y][local_id.x] = 0.0;
        }

        // For B_T: row = col (output column), col within tile = local_id.y
        let b_k_idx = (t * 16u) + local_id.y;
        if (col < dims.n && b_k_idx < dims.k) {
            tile_b[local_id.y][local_id.x] = matrix_b_t[(col * dims.k) + b_k_idx];
        } else {
            tile_b[local_id.y][local_id.x] = 0.0;
        }

        workgroupBarrier();

        // Compute partial dot product from shared memory
        for (var i: u32 = 0u; i < 16u; i = i + 1u) {
            accumulator += tile_a[local_id.y][i] * tile_b[i][local_id.x];
        }

        workgroupBarrier();
    }

    if (row < dims.m && col < dims.n) {
        matrix_c[(row * dims.n) + col] = accumulator;
    }
}
