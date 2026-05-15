// WGSL Stable Softmax with parallel max-reduction + normalization.
//
// Two-phase in one dispatch:
//   1. Find max across all scores (parallel reduction in shared memory)
//   2. Compute exp(score - max) and sum
//   3. Normalize: output[i] = exp(score[i] - max) / sum
//
// One workgroup processes one attention head's score vector.
// Workgroup size 256 handles sequences up to 256 directly;
// for longer sequences, each thread strides.

struct Params {
    seq_len: u32,   // Number of scores to softmax over
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> scores: array<f32>;
@group(0) @binding(1) var<storage, read_write> probs: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

var<workgroup> shared_max: array<f32, 256>;
var<workgroup> shared_sum: array<f32, 256>;

@compute @workgroup_size(256, 1, 1)
fn main(
    @builtin(local_invocation_id) lid: vec3<u32>,
    @builtin(workgroup_id) wid: vec3<u32>,
) {
    let tid = lid.x;
    let head_offset = wid.x * params.seq_len;

    // Phase 1: Find max (strided reduction)
    var local_max: f32 = -3.40282347e38;
    for (var i: u32 = tid; i < params.seq_len; i = i + 256u) {
        let val = scores[head_offset + i];
        local_max = max(local_max, val);
    }
    shared_max[tid] = local_max;
    workgroupBarrier();

    // Parallel reduction for max
    if (tid < 128u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 128u]); }
    workgroupBarrier();
    if (tid < 64u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 64u]); }
    workgroupBarrier();
    if (tid < 32u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 32u]); }
    workgroupBarrier();
    if (tid < 16u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 16u]); }
    workgroupBarrier();
    if (tid < 8u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 8u]); }
    workgroupBarrier();
    if (tid < 4u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 4u]); }
    workgroupBarrier();
    if (tid < 2u) { shared_max[tid] = max(shared_max[tid], shared_max[tid + 2u]); }
    workgroupBarrier();
    if (tid == 0u) { shared_max[0] = max(shared_max[0], shared_max[1]); }
    workgroupBarrier();

    let row_max = shared_max[0];

    // Phase 2: Compute exp(x - max) and local sum
    var local_sum: f32 = 0.0;
    for (var i: u32 = tid; i < params.seq_len; i = i + 256u) {
        let val = exp(scores[head_offset + i] - row_max);
        probs[head_offset + i] = val; // Store unnormalized exp
        local_sum += val;
    }
    shared_sum[tid] = local_sum;
    workgroupBarrier();

    // Parallel reduction for sum
    if (tid < 128u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 128u]; }
    workgroupBarrier();
    if (tid < 64u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 64u]; }
    workgroupBarrier();
    if (tid < 32u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 32u]; }
    workgroupBarrier();
    if (tid < 16u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 16u]; }
    workgroupBarrier();
    if (tid < 8u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 8u]; }
    workgroupBarrier();
    if (tid < 4u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 4u]; }
    workgroupBarrier();
    if (tid < 2u) { shared_sum[tid] = shared_sum[tid] + shared_sum[tid + 2u]; }
    workgroupBarrier();
    if (tid == 0u) { shared_sum[0] = shared_sum[0] + shared_sum[1]; }
    workgroupBarrier();

    let row_sum = shared_sum[0];
    let inv_sum = 1.0 / row_sum;

    // Phase 3: Normalize
    for (var i: u32 = tid; i < params.seq_len; i = i + 256u) {
        probs[head_offset + i] = probs[head_offset + i] * inv_sum;
    }
}
