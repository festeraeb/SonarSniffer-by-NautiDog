//! Transformer forward pass — the complete layer execution loop.
//!
//! Sequences all GPU shader dispatches for one transformer layer:
//!   RMSNorm → QKV Projection → RoPE → Attention → Residual Add →
//!   RMSNorm → Gate/Up Projection → SwiGLU → Down Projection → Residual Add
//!
//! Supports both single-buffer and chunked weight tensors transparently.
//! Designed for autoregressive decode (one token at a time against KV cache).

use bytemuck::{Pod, Zeroable};
use crate::shader_ops::{DequantPipeline, RmsNormPipeline, RmsNormParams};
use crate::tensor_chunker::ChunkedMatmulPipeline;
use crate::tensor_loader_safe::TensorRegistry;

/// All pipelines needed for one transformer layer.
pub struct LayerPipelines {
    pub rmsnorm: RmsNormPipeline,
    pub chunked_matmul: ChunkedMatmulPipeline,
    pub rope: wgpu::ComputePipeline,
    pub rope_bgl: wgpu::BindGroupLayout,
    pub attention: wgpu::ComputePipeline,
    pub attention_bgl: wgpu::BindGroupLayout,
    pub softmax: wgpu::ComputePipeline,
    pub softmax_bgl: wgpu::BindGroupLayout,
    pub swiglu: wgpu::ComputePipeline,
    pub swiglu_bgl: wgpu::BindGroupLayout,
    pub add: wgpu::ComputePipeline,
    pub add_bgl: wgpu::BindGroupLayout,
    pub av: wgpu::ComputePipeline,
    pub av_bgl: wgpu::BindGroupLayout,
    pub dequant_q6k: crate::shader_ops::DequantQ6KPipeline,
    pub transpose: wgpu::ComputePipeline,
    pub transpose_bgl: wgpu::BindGroupLayout,
    pub matvec: wgpu::ComputePipeline,
    pub matvec_bgl: wgpu::BindGroupLayout,
}

/// Uniform params for RoPE dispatch.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct RoPEParams {
    pub head_dim: u32,
    pub pos: u32,
    pub n_heads: u32,
    pub _pad: u32,
}

/// Uniform params for attention score computation.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AttentionParams {
    pub kv_len: u32,
    pub head_dim: u32,
    pub cur_pos: u32,
    pub scale: f32,
}

/// Uniform params for softmax.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SoftmaxParams {
    pub seq_len: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Uniform params for SwiGLU.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct SwiGLUParams {
    pub n_elements_vec4: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Uniform params for elementwise add.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AddParams {
    pub n_elements: u32,
    pub _pad0: u32,
    pub _pad1: u32,
    pub _pad2: u32,
}

/// Per-layer weight buffer references.
pub struct LayerWeights {
    pub attn_norm: wgpu::Buffer,    // RMSNorm weight [hidden_dim]
    pub ffn_norm: wgpu::Buffer,     // RMSNorm weight [hidden_dim]
    pub q_proj: wgpu::Buffer,       // [hidden_dim × hidden_dim]
    pub k_proj: wgpu::Buffer,       // [hidden_dim × kv_dim]
    pub v_proj: wgpu::Buffer,       // [hidden_dim × kv_dim]
    pub o_proj: wgpu::Buffer,       // [hidden_dim × hidden_dim]
    pub gate_proj: wgpu::Buffer,    // [intermediate × hidden_dim]
    pub up_proj: wgpu::Buffer,      // [intermediate × hidden_dim]
    pub down_proj: wgpu::Buffer,    // [hidden_dim × intermediate]
    // Qwen-specific biases (optional — None if model doesn't have them)
    pub q_bias: Option<wgpu::Buffer>,   // [hidden_dim]
    pub k_bias: Option<wgpu::Buffer>,   // [kv_dim]
    pub v_bias: Option<wgpu::Buffer>,   // [kv_dim]
}

/// KV cache for one layer (ring buffer on GPU).
pub struct KVCache {
    pub key_cache: wgpu::Buffer,    // [max_seq × n_kv_heads × head_dim]
    pub value_cache: wgpu::Buffer,  // [max_seq × n_kv_heads × head_dim]
    pub current_len: u32,
    pub max_seq: u32,
}

/// Model configuration.
#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub hidden_dim: u32,
    pub intermediate_dim: u32,
    pub n_heads: u32,
    pub n_kv_heads: u32,
    pub head_dim: u32,
    pub n_layers: u32,
    pub vocab_size: u32,
    pub max_seq_len: u32,
    pub rms_norm_eps: f32,
}

/// Execute one transformer layer (autoregressive decode, single token).
///
/// Input: `hidden_state` buffer [hidden_dim] f32
/// Output: `hidden_state` buffer updated in-place with layer output
pub fn execute_layer(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    config: &ModelConfig,
    weights: &LayerWeights,
    kv_cache: &mut KVCache,
    hidden_state: &wgpu::Buffer,
    pos: u32,
) {
    let hidden_bytes = (config.hidden_dim * 4) as u64;
    let intermediate_bytes = (config.intermediate_dim * 4) as u64;

    // ── Attention Block ─────────────────────────────────────────────────────

    // 1. RMSNorm(hidden_state) → normed
    let normed = create_temp_buffer(device, "normed", hidden_bytes);
    pipelines.rmsnorm.dispatch(
        device, queue, encoder,
        hidden_state, &weights.attn_norm, &normed,
        config.hidden_dim, 1, config.rms_norm_eps,
    );

    // 2. Q/K/V projections: normed × W_q, normed × W_k, normed × W_v
    let q_buf = create_temp_buffer(device, "q", hidden_bytes);
    let kv_dim_bytes = (config.n_kv_heads * config.head_dim * 4) as u64;
    let k_buf = create_temp_buffer(device, "k", kv_dim_bytes);
    let v_buf = create_temp_buffer(device, "v", kv_dim_bytes);

    // These use the matvec shader (GGUF native layout)
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &normed, &weights.q_proj, &q_buf,
        1, config.hidden_dim, config.hidden_dim);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &normed, &weights.k_proj, &k_buf,
        1, config.hidden_dim, config.n_kv_heads * config.head_dim);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &normed, &weights.v_proj, &v_buf,
        1, config.hidden_dim, config.n_kv_heads * config.head_dim);

    // Add QKV biases (Qwen-specific) — use temp buffers to avoid aliasing
    if let Some(ref q_bias) = weights.q_bias {
        let q_tmp = create_temp_buffer(device, "q_biased", hidden_bytes);
        dispatch_add(device, queue, encoder, pipelines,
            &q_buf, q_bias, &q_tmp, config.hidden_dim);
        encoder.copy_buffer_to_buffer(&q_tmp, 0, &q_buf, 0, hidden_bytes);
    }
    if let Some(ref k_bias) = weights.k_bias {
        let k_tmp = create_temp_buffer(device, "k_biased", kv_dim_bytes);
        dispatch_add(device, queue, encoder, pipelines,
            &k_buf, k_bias, &k_tmp, config.n_kv_heads * config.head_dim);
        encoder.copy_buffer_to_buffer(&k_tmp, 0, &k_buf, 0, kv_dim_bytes);
    }
    if let Some(ref v_bias) = weights.v_bias {
        let v_tmp = create_temp_buffer(device, "v_biased", kv_dim_bytes);
        dispatch_add(device, queue, encoder, pipelines,
            &v_buf, v_bias, &v_tmp, config.n_kv_heads * config.head_dim);
        encoder.copy_buffer_to_buffer(&v_tmp, 0, &v_buf, 0, kv_dim_bytes);
    }

    // 3. Apply RoPE to Q and K (DISABLED FOR DIAGNOSTIC)
    // dispatch_rope(device, queue, encoder, pipelines, &q_buf,
    //     config.head_dim, pos, config.n_heads);
    // dispatch_rope(device, queue, encoder, pipelines, &k_buf,
    //     config.head_dim, pos, config.n_kv_heads);

    // 4. Update KV cache (append K and V at position `pos`)
    let k_offset = (pos * config.n_kv_heads * config.head_dim * 4) as u64;
    let v_offset = k_offset;
    encoder.copy_buffer_to_buffer(&k_buf, 0, &kv_cache.key_cache, k_offset, kv_dim_bytes);
    encoder.copy_buffer_to_buffer(&v_buf, 0, &kv_cache.value_cache, v_offset, kv_dim_bytes);
    kv_cache.current_len = pos + 1;

    // 5. Attention scores + softmax + context (per head)
    let attn_output = create_temp_buffer(device, "attn_out", hidden_bytes);
    crate::attention_dispatch::dispatch_multihead_attention(
        device, queue, encoder,
        &pipelines.attention, &pipelines.attention_bgl,
        &pipelines.softmax, &pipelines.softmax_bgl,
        &pipelines.av, &pipelines.av_bgl,
        &q_buf,
        &kv_cache.key_cache,
        &kv_cache.value_cache,
        &attn_output,
        config.n_heads,
        config.n_kv_heads,
        config.head_dim,
        pos,
    );

    // 6. Output projection: attn_output × W_o → projected
    let attn_projected = create_temp_buffer(device, "attn_proj", hidden_bytes);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &attn_output, &weights.o_proj, &attn_projected,
        1, config.hidden_dim, config.hidden_dim);

    // 7. Residual add: DISABLED — skip attention contribution entirely
    //    hidden_state passes through unchanged to FFN
    // let residual_temp = create_temp_buffer(device, "res_attn", hidden_bytes);
    // dispatch_add(device, queue, encoder, pipelines,
    //     hidden_state, &attn_projected, &residual_temp, config.hidden_dim);
    // encoder.copy_buffer_to_buffer(&residual_temp, 0, hidden_state, 0, hidden_bytes);

    // ── FFN Block (DISABLED FOR DIAGNOSTIC) ───────────────────────────────
    // Skip FFN entirely — only RMSNorm + attention residual (also disabled) active

    // 8-12: FFN ENABLED FOR DIAGNOSTIC (1 layer only via generate.rs take(1))
    let ffn_normed = create_temp_buffer(device, "ffn_normed", hidden_bytes);
    pipelines.rmsnorm.dispatch(
        device, queue, encoder,
        hidden_state, &weights.ffn_norm, &ffn_normed,
        config.hidden_dim, 1, config.rms_norm_eps,
    );

    let gate_out = create_temp_buffer(device, "gate", intermediate_bytes);
    let up_out = create_temp_buffer(device, "up", intermediate_bytes);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &ffn_normed, &weights.gate_proj, &gate_out,
        1, config.hidden_dim, config.intermediate_dim);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &ffn_normed, &weights.up_proj, &up_out,
        1, config.hidden_dim, config.intermediate_dim);

    let ffn_activated = create_temp_buffer(device, "ffn_act", intermediate_bytes);
    dispatch_swiglu(device, queue, encoder, pipelines,
        &gate_out, &up_out, &ffn_activated, config.intermediate_dim);

    let ffn_out = create_temp_buffer(device, "ffn_out", hidden_bytes);
    dispatch_tiled_matmul(device, queue, encoder, pipelines,
        &ffn_activated, &weights.down_proj, &ffn_out,
        1, config.intermediate_dim, config.hidden_dim);

    let residual_temp2 = create_temp_buffer(device, "res_ffn", hidden_bytes);
    dispatch_add(device, queue, encoder, pipelines,
        hidden_state, &ffn_out, &residual_temp2, config.hidden_dim);
    encoder.copy_buffer_to_buffer(&residual_temp2, 0, hidden_state, 0, hidden_bytes);
}

// ── Dispatch helpers ────────────────────────────────────────────────────────

fn create_temp_buffer(device: &wgpu::Device, label: &str, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size,
        usage: wgpu::BufferUsages::STORAGE
            | wgpu::BufferUsages::COPY_SRC
            | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn dispatch_tiled_matmul(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    input: &wgpu::Buffer,
    weights: &wgpu::Buffer,
    output: &wgpu::Buffer,
    m: u32, k: u32, n: u32,
) {
    // For single-token decode (m=1), use the matvec shader which reads
    // weights in GGUF native [N × K] row-major layout directly.
    // output[n] = sum_k(W[n * K + k] * input[k])

    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct MatvecParams { n_out: u32, k_in: u32, _pad0: u32, _pad1: u32 }

    let params = MatvecParams { n_out: n, k_in: k, _pad0: 0, _pad1: 0 };
    let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("matvec_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buf, 0, bytemuck::cast_slice(&[params]));

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("matvec_bg"),
        layout: &pipelines.matvec_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: weights.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(&pipelines.matvec);
    pass.set_bind_group(0, Some(&bg), &[]);
    pass.dispatch_workgroups((n + 255) / 256, 1, 1);
}

fn dispatch_tiled_matmul_raw(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    input: &wgpu::Buffer,
    weights: &wgpu::Buffer,
    output: &wgpu::Buffer,
    m: u32, k: u32, n: u32,
) {
    #[repr(C)]
    #[derive(Clone, Copy, Pod, Zeroable)]
    struct TiledParams { m: u32, k: u32, n: u32, _pad: u32 }

    let params = TiledParams { m, k, n, _pad: 0 };
    let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("tiled_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buf, 0, bytemuck::cast_slice(&[params]));

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("tiled_bg"),
        layout: &pipelines.chunked_matmul.tiled_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: input.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: weights.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(&pipelines.chunked_matmul.tiled_pipeline);
    pass.set_bind_group(0, Some(&bg), &[]);
    pass.dispatch_workgroups((n + 15) / 16, (m + 15) / 16, 1);
}

fn dispatch_rope(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    qk_buf: &wgpu::Buffer,
    head_dim: u32,
    pos: u32,
    n_heads: u32,
) {
    let params = RoPEParams { head_dim, pos, n_heads, _pad: 0 };
    let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("rope_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buf, 0, bytemuck::cast_slice(&[params]));

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("rope_bg"),
        layout: &pipelines.rope_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: qk_buf.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(&pipelines.rope);
    pass.set_bind_group(0, Some(&bg), &[]);
    // One workgroup per head, 64 threads per workgroup (handles head_dim/2 pairs)
    pass.dispatch_workgroups(n_heads, 1, 1);
}

fn dispatch_swiglu(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    gate: &wgpu::Buffer,
    up: &wgpu::Buffer,
    output: &wgpu::Buffer,
    n_elements: u32,
) {
    let params = SwiGLUParams {
        n_elements_vec4: (n_elements + 3) / 4,
        _pad0: 0, _pad1: 0, _pad2: 0,
    };
    let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("swiglu_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buf, 0, bytemuck::cast_slice(&[params]));

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("swiglu_bg"),
        layout: &pipelines.swiglu_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: gate.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: up.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(&pipelines.swiglu);
    pass.set_bind_group(0, Some(&bg), &[]);
    pass.dispatch_workgroups((params.n_elements_vec4 + 255) / 256, 1, 1);
}

fn dispatch_add(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    pipelines: &LayerPipelines,
    a: &wgpu::Buffer,
    b: &wgpu::Buffer,
    output: &wgpu::Buffer,
    n_elements: u32,
) {
    let params = AddParams { n_elements, _pad0: 0, _pad1: 0, _pad2: 0 };
    let params_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("add_params"),
        size: 16,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&params_buf, 0, bytemuck::cast_slice(&[params]));

    let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("add_bg"),
        layout: &pipelines.add_bgl,
        entries: &[
            wgpu::BindGroupEntry { binding: 0, resource: a.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 1, resource: b.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: output.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 3, resource: params_buf.as_entire_binding() },
        ],
    });

    let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(&pipelines.add);
    pass.set_bind_group(0, Some(&bg), &[]);
    pass.dispatch_workgroups((n_elements + 255) / 256, 1, 1);
}
