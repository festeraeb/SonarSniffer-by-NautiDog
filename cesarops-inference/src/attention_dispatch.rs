//! Multi-head attention dispatch with correct KV cache stride for GQA.
//!
//! KV cache layout: [pos][n_kv_heads][head_dim]
//! For Qwen 1.5B: 12 query heads, 2 KV heads, head_dim=128
//! GQA group size = 12/2 = 6 (6 query heads share 1 KV head)

use bytemuck::{Pod, Zeroable};
use crate::forward_pass::SoftmaxParams;

/// Params for the attention QK^T shader (with KV stride).
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AttnQKParams {
    pub kv_len: u32,
    pub head_dim: u32,
    pub cur_pos: u32,
    pub scale: f32,
    pub kv_stride: u32,        // n_kv_heads * head_dim (elements between positions)
    pub kv_head_offset: u32,   // kv_head * head_dim (offset to this head within a position)
    pub _pad0: u32,
    pub _pad1: u32,
}

/// Params for the attention-value weighted sum shader.
#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
pub struct AVParams {
    pub kv_len: u32,
    pub head_dim: u32,
    pub kv_stride: u32,
    pub kv_head_offset: u32,
}

/// Dispatch multi-head attention for a single decode step.
pub fn dispatch_multihead_attention(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    encoder: &mut wgpu::CommandEncoder,
    attn_pipeline: &wgpu::ComputePipeline,
    attn_bgl: &wgpu::BindGroupLayout,
    softmax_pipeline: &wgpu::ComputePipeline,
    softmax_bgl: &wgpu::BindGroupLayout,
    av_pipeline: &wgpu::ComputePipeline,
    av_bgl: &wgpu::BindGroupLayout,
    q_buf: &wgpu::Buffer,
    kv_cache_k: &wgpu::Buffer,
    kv_cache_v: &wgpu::Buffer,
    output_buf: &wgpu::Buffer,
    n_heads: u32,
    n_kv_heads: u32,
    head_dim: u32,
    cur_pos: u32,
) {
    let kv_len = cur_pos + 1;
    let scale = 1.0 / (head_dim as f32).sqrt();
    let heads_per_kv = n_heads / n_kv_heads; // GQA ratio (6 for Qwen 1.5B)
    let kv_stride = n_kv_heads * head_dim;   // Elements between positions in KV cache

    // Scratch buffers
    let scores_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("attn_scores"),
        size: (kv_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let probs_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("attn_probs"),
        size: (kv_len * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    for h in 0..n_heads {
        let kv_head = h / heads_per_kv;
        let q_offset = (h * head_dim * 4) as u64;
        let out_offset = (h * head_dim * 4) as u64;
        let kv_head_offset = kv_head * head_dim; // Element offset to this KV head

        // ── QK^T ────────────────────────────────────────────────────────────
        let qk_params = AttnQKParams {
            kv_len,
            head_dim,
            cur_pos,
            scale,
            kv_stride,
            kv_head_offset,
            _pad0: 0,
            _pad1: 0,
        };
        let qk_params_buf = create_uniform(device, queue, &qk_params);

        let qk_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("attn_qk_bg"),
            layout: attn_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: q_buf,
                        offset: q_offset,
                        size: wgpu::BufferSize::new((head_dim * 4) as u64),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: kv_cache_k.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: scores_buf.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: qk_params_buf.as_entire_binding(),
                },
            ],
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(attn_pipeline);
            pass.set_bind_group(0, Some(&qk_bg), &[]);
            pass.dispatch_workgroups((kv_len + 255) / 256, 1, 1);
        }

        // ── Softmax ─────────────────────────────────────────────────────────
        let softmax_params = SoftmaxParams {
            seq_len: kv_len,
            _pad0: 0,
            _pad1: 0,
            _pad2: 0,
        };
        let softmax_params_buf = create_uniform(device, queue, &softmax_params);

        let softmax_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("softmax_bg"),
            layout: softmax_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: scores_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: probs_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: softmax_params_buf.as_entire_binding() },
            ],
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(softmax_pipeline);
            pass.set_bind_group(0, Some(&softmax_bg), &[]);
            pass.dispatch_workgroups(1, 1, 1);
        }

        // ── AV weighted sum ─────────────────────────────────────────────────
        let av_params = AVParams {
            kv_len,
            head_dim,
            kv_stride,
            kv_head_offset,
        };
        let av_params_buf = create_uniform(device, queue, &av_params);

        let av_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("av_bg"),
            layout: av_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: probs_buf.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: kv_cache_v.as_entire_binding() },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: output_buf,
                        offset: out_offset,
                        size: wgpu::BufferSize::new((head_dim * 4) as u64),
                    }),
                },
                wgpu::BindGroupEntry { binding: 3, resource: av_params_buf.as_entire_binding() },
            ],
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(av_pipeline);
            pass.set_bind_group(0, Some(&av_bg), &[]);
            pass.dispatch_workgroups((head_dim + 255) / 256, 1, 1);
        }
    }
}

fn create_uniform<T: Pod>(device: &wgpu::Device, queue: &wgpu::Queue, data: &T) -> wgpu::Buffer {
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniform"),
        size: std::mem::size_of::<T>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, bytemuck::cast_slice(&[*data]));
    buf
}


/// Dispatch multi-head attention with separate submits per head.
/// Avoids buffer reuse sync issues on P100 Vulkan.
pub fn dispatch_multihead_attention_split(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    attn_pipeline: &wgpu::ComputePipeline,
    attn_bgl: &wgpu::BindGroupLayout,
    softmax_pipeline: &wgpu::ComputePipeline,
    softmax_bgl: &wgpu::BindGroupLayout,
    av_pipeline: &wgpu::ComputePipeline,
    av_bgl: &wgpu::BindGroupLayout,
    q_buf: &wgpu::Buffer,
    kv_cache_k: &wgpu::Buffer,
    kv_cache_v: &wgpu::Buffer,
    output_buf: &wgpu::Buffer,
    n_heads: u32,
    n_kv_heads: u32,
    head_dim: u32,
    cur_pos: u32,
) {
    let kv_len = cur_pos + 1;
    let scale = 1.0 / (head_dim as f32).sqrt();
    let heads_per_kv = n_heads / n_kv_heads;
    let kv_stride = n_kv_heads * head_dim;

    for h in 0..n_heads {
        let kv_head = h / heads_per_kv;
        let q_offset = (h * head_dim * 4) as u64;
        let out_offset = (h * head_dim * 4) as u64;
        let kv_head_offset = kv_head * head_dim;

        // Fresh buffers per head to avoid reuse issues
        let scores_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("attn_scores"),
            size: (kv_len * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let probs_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("attn_probs"),
            size: (kv_len * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // QK^T
        let qk_params = AttnQKParams {
            kv_len, head_dim, cur_pos, scale, kv_stride, kv_head_offset, _pad0: 0, _pad1: 0,
        };
        let qk_params_buf = create_uniform(device, queue, &qk_params);
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let qk_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("attn_qk_bg"),
                layout: attn_bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: q_buf, offset: q_offset,
                            size: wgpu::BufferSize::new((head_dim * 4) as u64),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: kv_cache_k.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: scores_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: qk_params_buf.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(attn_pipeline);
            pass.set_bind_group(0, Some(&qk_bg), &[]);
            pass.dispatch_workgroups((kv_len + 255) / 256, 1, 1);
            drop(pass);
            queue.submit(std::iter::once(enc.finish()));
        }

        // Softmax
        let softmax_params = crate::forward_pass::SoftmaxParams {
            seq_len: kv_len, _pad0: 0, _pad1: 0, _pad2: 0,
        };
        let softmax_params_buf = create_uniform(device, queue, &softmax_params);
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let softmax_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("softmax_bg"),
                layout: softmax_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: scores_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: probs_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: softmax_params_buf.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(softmax_pipeline);
            pass.set_bind_group(0, Some(&softmax_bg), &[]);
            pass.dispatch_workgroups(1, 1, 1);
            drop(pass);
            queue.submit(std::iter::once(enc.finish()));
        }

        // AV weighted sum
        let av_params = AVParams { kv_len, head_dim, kv_stride, kv_head_offset };
        let av_params_buf = create_uniform(device, queue, &av_params);
        {
            let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            let av_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("av_bg"),
                layout: av_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: probs_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 1, resource: kv_cache_v.as_entire_binding() },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: output_buf, offset: out_offset,
                            size: wgpu::BufferSize::new((head_dim * 4) as u64),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 3, resource: av_params_buf.as_entire_binding() },
                ],
            });
            let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
            pass.set_pipeline(av_pipeline);
            pass.set_bind_group(0, Some(&av_bg), &[]);
            pass.dispatch_workgroups((head_dim + 255) / 256, 1, 1);
            drop(pass);
            queue.submit(std::iter::once(enc.finish()));
        }
    }
}
