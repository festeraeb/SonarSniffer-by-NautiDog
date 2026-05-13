// FlashAttention-2 for P100 (SM 6.0)
// Tiled attention using Workgroup Shared Memory — O(N) memory, not O(N²)
// FP16 inputs/outputs for 2:1 throughput, f32 softmax for numerical stability
// 16x16 workgroup — matches matmul_half2.wgsl for consistent occupancy
// Register pressure: ~8 vars per thread (well under 32 limit)

enable f16;

struct Params {
    seq_len: u32,
    head_dim: u32,
    n_heads: u32,
    scale: f16,
};

@group(0) @binding(0) var<storage, read> Q: array<f16>;
@group(0) @binding(1) var<storage, read> K: array<f16>;
@group(0) @binding(2) var<storage, read> V: array<f16>;
@group(0) @binding(3) var<storage, read_write> O: array<f16>;
@group(0) @binding(4) var<uniform> params: Params;

// Workgroup shared memory for tiled Q, K, V blocks
// Each tile: 16x16 = 256 elements × 2 bytes = 512 bytes
// Total shared: ~1.5KB — well within P100's 48KB limit per block
var<workgroup> q_tile: array<f16, 256>; // 16x16 block
var<workgroup> k_tile: array<f16, 256>;
var<workgroup> v_tile: array<f16, 256>;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>,
        @builtin(local_invocation_id) local_id: vec3<u32>) {
    let row = global_id.y;
    let col = global_id.x;
    let local_row = local_id.y;
    let local_col = local_id.x;

    // Boundary check
    if (row >= params.seq_len || col >= params.head_dim) {
        return;
    }

    // Running statistics for online softmax (f32 for numerical stability)
    var m_prev: f32 = -1e38;
    var l_prev: f32 = 0.0;
    var o_acc: f32 = 0.0;

    // Tiling loop over K/V sequence blocks
    for (var b = 0u; b < params.seq_len; b += 16u) {
        // Cooperative load into Workgroup Shared Memory (SRAM)
        // All 256 threads participate — coalesced memory access
        q_tile[local_row * 16u + local_col] = Q[row * params.head_dim + local_col];
        k_tile[local_row * 16u + local_col] = K[(b + local_row) * params.head_dim + local_col];
        v_tile[local_row * 16u + local_col] = V[(b + local_row) * params.head_dim + local_col];
        workgroupBarrier();

        // Compute local attention score: S = Q_row · K_col^T (dot product)
        var s: f32 = 0.0;
        for (var k = 0u; k < 16u; k++) {
            s += f32(q_tile[local_row * 16u + k] * k_tile[k * 16u + local_col]);
        }
        s *= f32(params.scale);

        // Online softmax update (FlashAttention-2 algorithm)
        let m_curr = max(m_prev, s);
        let p = exp(s - m_curr);
        let l_curr = exp(m_prev - m_curr) * l_prev + p;

        // Rescale output accumulator and add new contribution
        o_acc = o_acc * exp(m_prev - m_curr) + p * f32(v_tile[local_row * 16u + local_col]);

        // Update running stats
        m_prev = m_curr;
        l_prev = l_curr;
        workgroupBarrier();
    }

    // Final normalization and write to global memory
    O[row * params.head_dim + col] = f16(o_acc / l_prev);
}
