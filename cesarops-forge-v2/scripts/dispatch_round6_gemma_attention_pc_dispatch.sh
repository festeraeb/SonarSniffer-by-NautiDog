#!/bin/bash
# Dispatch to Gemma-4-MoE on P100 #0 (port 5001).
# Task #1: Wire attention_pc into dispatch_multihead_attention_split.
# We already have attention_pc shader + pipeline shipping (round 4). Need the
# dispatch helper to use it when Some(attention_pc).
set -e
OUT_DIR="/home/cesarops/wreckhunter2000-1/cesarops-forge-v2/dispatch_results"
mkdir -p "$OUT_DIR"

cat > "$OUT_DIR/r6_gemma_attention_pc.prompt" << 'PROMPT_EOF'
Output one Rust function as a complete replacement for the existing
`dispatch_multihead_attention_split` in `attention_dispatch.rs`. NO commentary,
NO markdown fences, NO <think> blocks.

CURRENT SIGNATURE (keep it identical):

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
)

WHAT TO CHANGE: add two NEW optional parameters at the end so the call site
can pass the push-constant attention pipeline + bgl when available. The new
signature must be:

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
    attn_pc_pipeline: Option<&wgpu::ComputePipeline>,
    attn_pc_bgl: Option<&wgpu::BindGroupLayout>,
)

INSIDE THE FUNCTION:
- Keep the per-head loop (each head gets fresh scores_buf + probs_buf).
- For the QK^T submit ONLY: when both attn_pc_pipeline and attn_pc_bgl are
  Some(_), use the push-constant path. Otherwise fall back to the existing
  uniform-buffer path you already have.

PUSH-CONSTANT QK PATH (use this when both Some):

    let qk_params = AttnQKParams {
        kv_len, head_dim, cur_pos, scale, kv_stride, kv_head_offset,
        _pad0: 0, _pad1: 0,
    };
    let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
    let qk_bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("attn_pc_qk_bg"),
        layout: pc_bgl,    // 3 bindings only: query, key_cache, scores
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: q_buf,
                    offset: q_offset,
                    size: wgpu::BufferSize::new((head_dim * 4) as u64),
                }),
            },
            wgpu::BindGroupEntry { binding: 1, resource: kv_cache_k.as_entire_binding() },
            wgpu::BindGroupEntry { binding: 2, resource: scores_buf.as_entire_binding() },
        ],
    });
    let mut pass = enc.begin_compute_pass(&wgpu::ComputePassDescriptor::default());
    pass.set_pipeline(pc_pipeline);
    pass.set_bind_group(0, Some(&qk_bg), &[]);
    pass.set_push_constants(0, bytemuck::cast_slice(&[qk_params]));
    pass.dispatch_workgroups((kv_len + 255) / 256, 1, 1);
    drop(pass);
    queue.submit(std::iter::once(enc.finish()));

The Softmax + AV stages stay UNCHANGED (uniform-buffer path).
Keep all existing diagnostic readback for h==0 && kv_len==2.

Return ONLY the complete function body. No commentary. No fences.
PROMPT_EOF

PAYLOAD=$(python3 -c "import json; p=open('$OUT_DIR/r6_gemma_attention_pc.prompt').read(); print(json.dumps({'prompt': p, 'max_length': 3000, 'temperature': 0.15, 'top_p': 0.9, 'rep_pen': 1.05}))")

curl -s -m 600 -X POST http://127.0.0.1:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d "$PAYLOAD" \
  > "$OUT_DIR/r6_gemma_attention_pc.json" 2>&1

python3 -c "import json; d=json.load(open('$OUT_DIR/r6_gemma_attention_pc.json')); print(d.get('results',[{}])[0].get('text',''))" \
  > "$OUT_DIR/r6_gemma_attention_pc.rs" 2>"$OUT_DIR/r6_gemma_attention_pc.err"
echo "[Gemma-4 r6] attention_pc dispatch: $(wc -l < "$OUT_DIR/r6_gemma_attention_pc.rs") lines"
