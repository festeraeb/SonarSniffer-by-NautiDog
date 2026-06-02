//! Single-GPU runner for Gemma-4-26B-MoE.
//!
//! Wires together:
//!   * `Gemma4Layer`        — attention + dense FFN bypass
//!   * `MoeFfnDispatch`     — MoE expert FFN (gate_up + silu·up + down + accum)
//!   * `Iq4MatvecPipeline`  — shared GPU matvec primitives
//!   * GGUF metadata        — per-layer head_count_kv + sliding_window_pattern
//!   * LM head + softcap    — final logit projection with `30 * tanh(x/30)`
//!
//! What this file does NOT do (called out, not silently skipped):
//!   * Multi-GPU dispatch — single GPU only. PlacementPlan integration
//!     is a follow-up.
//!   * Tokenization — `ZeroAllocBpeTokenizer` is Qwen-tuned. The runner
//!     accepts already-tokenized prompts as `Vec<u32>`. A gemma4 tokenizer
//!     is its own task.
//!   * GPU softmax sampling — small CPU readback of 262144 logits per token.
//!
//! Honest gaps that the operator must verify against llama.cpp output:
//!   1. Order of `pre_ffw_norm_2`, `post_ffw_norm_1`, `post_ffw_norm_2`
//!      in the MoE block. We follow the spec doc as written; the candle
//!      reference doesn't model MoE. If output diverges from llama.cpp,
//!      this is the first place to look.
//!   2. Gate/up split direction inside the 1408 row dim of
//!      `ffn_gate_up_exps`. We assume rows 0..704 = gate, 704..1408 = up.
//!      Flipped via `silu_mul_split_f32.wgsl`.

use std::path::Path;
use std::sync::Arc;

use crate::bridge;
use crate::gemma4_layer::{Gemma4Config, Gemma4Layer};
use crate::hardware;
use crate::iq4_pipeline::{
    iq4nl_shader_src, iq4xs_shader_src, Iq4MatvecPipeline, MatvecPush, QuantKind,
};
use crate::kv_cache::KvCache;
use crate::loader::{self, GgufValue, ModelWeights};
use crate::moe_iq4_dispatch::{MoeFfnConfig, MoeFfnDispatch};

// ─────────────────────────────────────────────────────────────────────────────
// fp32 row-major matvec pipeline (used by the LM head)
// ─────────────────────────────────────────────────────────────────────────────

#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
struct F32MatvecPush {
    k: u32,
    n: u32,
    _pad0: u32,
    _pad1: u32,
}

pub struct F32MatvecPipeline {
    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
    pub pipeline: wgpu::ComputePipeline,
    pub bgl: wgpu::BindGroupLayout,
}

impl F32MatvecPipeline {
    pub fn new(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("matvec_f32_rowmajor"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("../shaders/matvec_f32_rowmajor.wgsl").into(),
            ),
        });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("f32_matvec_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: true },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false, min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("f32_matvec_pl"),
            bind_group_layouts: &[&bgl],
            push_constant_ranges: &[],
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("f32_matvec_pipeline"),
            layout: Some(&pl),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        Self { device, queue, pipeline, bgl }
    }

    /// Run y[n] = W[n,k] @ x[k]. All buffers are GPU-resident.
    /// Encodes into the supplied encoder. Caller submits + reads back.
    pub fn dispatch(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        w: &wgpu::Buffer,
        x: &wgpu::Buffer,
        y: &wgpu::Buffer,
        push: &wgpu::Buffer,
        n_rows: u32,
    ) {
        let bg = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("f32_matvec_bg"),
            layout: &self.bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: w.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: x.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: y.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: push.as_entire_binding() },
            ],
        });
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("f32_matvec_pass"),
            timestamp_writes: None,
        });
        pass.set_pipeline(&self.pipeline);
        pass.set_bind_group(0, Some(&bg), &[]);
        pass.dispatch_workgroups(n_rows, 1, 1);
    }

    pub fn make_push_buffer(&self, k: u32, n: u32) -> wgpu::Buffer {
        let buf = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("f32_matvec_push"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buf, 0, bytemuck::bytes_of(&F32MatvecPush {
            k, n, _pad0: 0, _pad1: 0,
        }));
        buf
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Top-level runner
// ─────────────────────────────────────────────────────────────────────────────

pub struct Gemma4Runner {
    pub config: Gemma4Config,
    pub moe_config: MoeFfnConfig,
    pub layers: Vec<Gemma4LayerWithMoe>,

    /// Token embedding table — kept on CPU as f32 for cheap row lookup.
    /// Q6_K dequantized once at load. ~3 GB for vocab=262144 × hidden=2816.
    pub token_embeddings: Vec<f32>,
    pub final_norm: Vec<f32>,
    /// Logit projection. We dequantize the LM head ONCE at load and upload
    /// to the GPU as fp32. Split into two buffers because the full matrix
    /// (262144 × 2816 × 4 = ~2.95 GB) exceeds the P100's 2 GB per-binding
    /// limit. Each half is ~1.47 GB.
    pub lm_head_f32_lo: wgpu::Buffer,  // rows 0..vocab/2
    pub lm_head_f32_hi: wgpu::Buffer,  // rows vocab/2..vocab
    pub lm_head_uses_tied_embedding: bool,
    pub logit_softcap: f32,
    pub vocab_size: usize,

    pub iq4xs_pipe: Arc<Iq4MatvecPipeline>,
    pub iq4nl_pipe: Arc<Iq4MatvecPipeline>,
    pub moe_dispatch: Arc<MoeFfnDispatch>,

    /// Generic fp32 row-major matvec pipeline. Used today by the LM head;
    /// future use cases (router, debug paths) can share it.
    pub f32_matvec: F32MatvecPipeline,

    pub device: Arc<wgpu::Device>,
    pub queue: Arc<wgpu::Queue>,
}

pub struct Gemma4LayerWithMoe {
    pub dense: Gemma4Layer,
    pub pre_ffw_norm_2: Vec<f32>,
    pub post_ffw_norm_1: Vec<f32>,
    pub post_ffw_norm_2: Vec<f32>,

    /// Slab-packed across all 128 experts (verified via `probe_moe_layout`).
    pub gate_up_exps: wgpu::Buffer,
    pub down_exps: wgpu::Buffer,
    pub router_w: wgpu::Buffer,
    pub router_scale: wgpu::Buffer,
}

#[derive(Debug, Clone)]
pub struct SamplingParams {
    pub max_new_tokens: usize,
    pub temperature: f32,
    pub top_p: f32,
    pub stop_tokens: Vec<u32>,
}

impl Default for SamplingParams {
    fn default() -> Self {
        Self {
            max_new_tokens: 64,
            temperature: 0.0, // greedy by default for first smoke test
            top_p: 1.0,
            stop_tokens: vec![1, 2], // Gemma-4 EOS=1, BOS=2
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Construction
// ─────────────────────────────────────────────────────────────────────────────

impl Gemma4Runner {
    /// Load a Gemma-4-26B-MoE checkpoint and prepare every weight for inference.
    ///
    /// Walks the GGUF, derives `Gemma4Config` from `gemma4.*` metadata,
    /// validates per-layer KV head counts and SWA pattern, and uploads
    /// every projection / expert tensor to the chosen GPU.
    pub fn load(model_path: &Path, gpu_index: usize) -> Result<Self, String> {
        let profile = hardware::audit_system();
        let weights =
            loader::load(model_path, &profile).map_err(|e| format!("loader::load: {e}"))?;

        // ── Resolve Gemma-4 config from GGUF metadata ─────────────────────
        let cfg = read_gemma4_config(&weights)?;
        let moe_cfg = read_moe_config(&weights)?;
        let n_layers = weights.n_layers;
        let head_count_kv_per_layer = read_head_count_kv_array(&weights, n_layers)?;
        let swa_pattern = read_swa_pattern(&weights, n_layers)?;
        let logit_softcap =
            get_f32_meta(&weights, "gemma4.final_logit_softcapping").unwrap_or(30.0);

        // ── Init GPU + pipelines ─────────────────────────────────────────
        let (device, queue) = init_device(gpu_index)?;

        let iq4xs_pipe = Arc::new(Iq4MatvecPipeline::new(
            device.clone(),
            queue.clone(),
            QuantKind::Iq4Xs,
            iq4xs_shader_src(),
        ));
        let iq4nl_pipe = Arc::new(Iq4MatvecPipeline::new(
            device.clone(),
            queue.clone(),
            QuantKind::Iq4Nl,
            iq4nl_shader_src(),
        ));
        let moe_dispatch =
            Arc::new(MoeFfnDispatch::new(device.clone(), queue.clone()));

        // ── Top-level CPU norms + embeddings ──────────────────────────────
        let token_embeddings = load_dequantized_or_fail(
            &weights,
            "token_embd.weight",
            cfg.hidden_size * read_vocab_size(&weights)?,
        )?;
        let final_norm =
            load_dequantized_or_fail(&weights, "output_norm.weight", cfg.hidden_size)?;

        // LM head: try output.weight (untied), fall back to token_embd.weight (tied).
        let (lm_head_f32_vec, lm_head_uses_tied) = if weights.tensors.contains_key("output.weight")
        {
            let region = weights.tensors.get("output.weight").unwrap();
            let n_elem: usize = region.shape.iter().product();
            (
                load_dequantized_or_fail(&weights, "output.weight", n_elem)?,
                false,
            )
        } else {
            tracing::info!("LM head tied to token_embd.weight (no separate output.weight)");
            (token_embeddings.clone(), true)
        };

        // Upload LM head as fp32 GPU buffer, split into two halves to stay
        // under the 2 GB per-binding limit on P100.
        let half_vocab = lm_head_f32_vec.len() / (2 * cfg.hidden_size);
        let half_elems = half_vocab * cfg.hidden_size;
        let half_bytes = (half_elems * 4) as u64;

        let lm_head_f32_lo = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gemma4.lm_head_f32_lo"),
            size: half_bytes,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &lm_head_f32_lo,
            0,
            bytemuck::cast_slice(&lm_head_f32_vec[..half_elems]),
        );

        let lm_head_f32_hi = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gemma4.lm_head_f32_hi"),
            size: (lm_head_f32_vec.len() - half_elems) as u64 * 4,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(
            &lm_head_f32_hi,
            0,
            bytemuck::cast_slice(&lm_head_f32_vec[half_elems..]),
        );

        // ── Per-layer construction ───────────────────────────────────────
        let mut layers = Vec::with_capacity(n_layers);
        for layer_idx in 0..n_layers {
            let is_swa = swa_pattern[layer_idx];
            let n_kv_heads = head_count_kv_per_layer[layer_idx];

            let dense = Gemma4Layer::load(
                layer_idx,
                is_swa,
                n_kv_heads,
                &cfg,
                &weights,
                &device,
                &queue,
            )?;

            let pre_ffw_norm_2 = load_norm_for_layer(
                &weights,
                layer_idx,
                "pre_ffw_norm_2",
                cfg.hidden_size,
            )?;
            let post_ffw_norm_1 = load_norm_for_layer(
                &weights,
                layer_idx,
                "post_ffw_norm_1",
                cfg.hidden_size,
            )?;
            let post_ffw_norm_2 = load_norm_for_layer(
                &weights,
                layer_idx,
                "post_ffw_norm_2",
                cfg.hidden_size,
            )?;

            let gate_up_exps = upload_raw_quant(
                &weights,
                &format!("blk.{}.ffn_gate_up_exps.weight", layer_idx),
                23, // IQ4_XS
                &device,
                &queue,
                "gemma4.ffn_gate_up_exps",
            )?;
            let down_exps = upload_raw_quant(
                &weights,
                &format!("blk.{}.ffn_down_exps.weight", layer_idx),
                20, // IQ4_NL
                &device,
                &queue,
                "gemma4.ffn_down_exps",
            )?;

            let router_w = upload_fp32_tensor(
                &weights,
                &format!("blk.{}.ffn_gate_inp.weight", layer_idx),
                cfg.hidden_size * moe_cfg.n_experts,
                &device,
                &queue,
                "gemma4.ffn_gate_inp.weight",
            )?;
            let router_scale = upload_fp32_tensor(
                &weights,
                &format!("blk.{}.ffn_gate_inp.scale", layer_idx),
                cfg.hidden_size,
                &device,
                &queue,
                "gemma4.ffn_gate_inp.scale",
            )?;

            layers.push(Gemma4LayerWithMoe {
                dense,
                pre_ffw_norm_2,
                post_ffw_norm_1,
                post_ffw_norm_2,
                gate_up_exps,
                down_exps,
                router_w,
                router_scale,
            });

            if layer_idx % 5 == 0 {
                tracing::info!(
                    "Loaded layer {}/{} (swa={}, n_kv_heads={})",
                    layer_idx + 1,
                    n_layers,
                    is_swa,
                    n_kv_heads
                );
            }
        }

        Ok(Self {
            config: cfg,
            moe_config: moe_cfg,
            layers,
            token_embeddings,
            final_norm,
            lm_head_f32_lo,
            lm_head_f32_hi,
            lm_head_uses_tied_embedding: lm_head_uses_tied,
            logit_softcap,
            vocab_size: read_vocab_size(&weights)?,
            iq4xs_pipe,
            iq4nl_pipe,
            moe_dispatch,
            f32_matvec: F32MatvecPipeline::new(device.clone(), queue.clone()),
            device,
            queue,
        })
    }

    // ── Forward pass for one token ───────────────────────────────────────
    ///
    /// `hidden` is read from `token_embeddings` for the input token id,
    /// then passed through every layer. Returns logits (vocab_size floats)
    /// after final_norm + LM head + softcap.
    pub fn forward_token(
        &self,
        token_id: u32,
        position: usize,
        kv_cache: &mut KvCache,
    ) -> Vec<f32> {
        let h = self.config.hidden_size;
        let vocab = self.vocab_size;

        // 1. Embedding row lookup (CPU).
        let row_start = (token_id as usize) * h;
        let row_end = row_start + h;
        let mut hidden_state = self.token_embeddings[row_start..row_end].to_vec();

        // Gemma embedding scale: x *= sqrt(hidden_size). This is a fixed
        // architectural constant for Gemma 1/2/3/4 (not stored in GGUF
        // metadata) and is critical for correct downstream attention/MLP
        // numerics. Without it the residual stream is ~53× too small for
        // hidden=2816, which propagates corruption through every layer.
        let embed_scale = (h as f32).sqrt();
        for v in hidden_state.iter_mut() {
            *v *= embed_scale;
        }

        // 2. Walk all layers.
        for layer in &self.layers {
            // Dense block (attention + dense FFN bypass).
            layer.dense.forward(
                &self.config,
                &mut hidden_state,
                position,
                kv_cache,
                &self.iq4xs_pipe,
                &self.iq4nl_pipe,
            );

            // MoE FFN block, sharing the same residual stream.
            // HONEST GAP (1): order of {pre_ffw_norm_2, post_ffw_norm_1,
            // post_ffw_norm_2} not pinned by spec doc. We follow the
            // pseudocode in the original task brief verbatim:
            //   residual = hidden
            //   hidden = pre_ffw_norm_2(hidden)
            //   moe_out = moe_dispatch.forward(hidden)
            //   moe_out = post_ffw_norm_1(moe_out)
            //   hidden = residual + layer_output_scale * moe_out
            //   hidden = post_ffw_norm_2(hidden)   // applied to summed residual
            self.moe_block(layer, &mut hidden_state);
        }

        // 3. Final RMSNorm (Gemma-style).
        rmsnorm_gemma_inplace(&mut hidden_state, &self.final_norm, self.config.rms_norm_eps);

        // 4. LM head matvec on GPU (fp32 path).
        let logits = self.lm_head_matvec(&hidden_state, h, vocab);

        // 5. Logit softcap on CPU.
        let cap = self.logit_softcap;
        let mut out = Vec::with_capacity(logits.len());
        for l in &logits {
            let v = (l / cap).tanh() * cap;
            out.push(v);
        }
        out
    }

    /// MoE FFN sub-block: pre-norm → dispatch → post-norm-1 → scaled residual
    /// → post-norm-2. Mutates `hidden_state` in place.
    fn moe_block(&self, layer: &Gemma4LayerWithMoe, hidden_state: &mut Vec<f32>) {
        let cfg = &self.config;
        let h = cfg.hidden_size;
        let eps = cfg.rms_norm_eps;
        let scale = layer.dense.layer_output_scale;

        let residual = hidden_state.clone();
        rmsnorm_gemma_inplace(hidden_state, &layer.pre_ffw_norm_2, eps);

        // GPU: upload x → run MoE dispatch → read y back to CPU.
        // The MoE dispatch wants `x_gpu` and `y_gpu` as wgpu::Buffer; we
        // create scratch buffers per call. Optimization opportunity: keep
        // these scratch around and reuse, but per-token alloc is fine for
        // a smoke test.
        let mut moe_out = vec![0.0f32; h];
        let device = &self.device;
        let queue = &self.queue;

        let x_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("moe_x_in"),
            size: (h * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&x_buf, 0, bytemuck::cast_slice(hidden_state));

        let y_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("moe_y_out"),
            size: (h * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE
                | wgpu::BufferUsages::COPY_DST
                | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        // Pre-zero y_buf — the MoE accumulator expects zeros.
        let zeros = vec![0u8; (h * 4) as usize];
        queue.write_buffer(&y_buf, 0, &zeros);

        // Dispatch the MoE FFN. This blocks on a tiny readback (128
        // logits) for top-k routing inside the dispatcher.
        if let Err(e) = self.moe_dispatch.forward(
            &self.moe_config,
            &x_buf,
            &layer.gate_up_exps,
            &layer.down_exps,
            &layer.router_w,
            &layer.router_scale,
            &y_buf,
        ) {
            tracing::error!("MoE forward failed: {}", e);
            return;
        }

        // Read y_buf back to CPU.
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("moe_y_staging"),
            size: (h * 4) as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut enc = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("moe_y_copy"),
        });
        enc.copy_buffer_to_buffer(&y_buf, 0, &staging, 0, (h * 4) as u64);
        let sub_idx = queue.submit(std::iter::once(enc.finish()));

        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| {
            let _ = tx.send(r);
        });
        device.poll(wgpu::Maintain::WaitForSubmissionIndex(sub_idx));
        let _ = rx.recv();
        let mapped = slice.get_mapped_range();
        moe_out.copy_from_slice(bytemuck::cast_slice(&mapped));
        drop(mapped);
        staging.unmap();

        // post_ffw_norm_1 on the MoE output before residual add.
        rmsnorm_gemma_inplace(&mut moe_out, &layer.post_ffw_norm_1, eps);

        // Residual + scaled MoE output.
        for i in 0..h {
            hidden_state[i] = residual[i] + scale * moe_out[i];
        }

        // post_ffw_norm_2 on the merged residual (Gemma "double norm" pattern).
        rmsnorm_gemma_inplace(hidden_state, &layer.post_ffw_norm_2, eps);
    }

    /// GPU fp32 LM head matvec, chunked to stay under both the 2 GB
    /// per-binding limit AND the 65535 max workgroups-per-dimension limit.
    /// We split into 4 chunks of ~65536 rows each.
    pub(crate) fn lm_head_matvec(&self, hidden: &[f32], k: usize, n: usize) -> Vec<f32> {
        let device = &self.device;
        let queue = &self.queue;
        const MAX_WG: usize = 65535;

        // Upload hidden state (small: ~11 KB).
        let x_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lm_head_x"),
            size: (k * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        queue.write_buffer(&x_buf, 0, bytemuck::cast_slice(hidden));

        // Split vocab into chunks that fit both the binding limit and dispatch limit.
        // lm_head_f32_lo covers rows 0..half_n, lm_head_f32_hi covers half_n..n.
        let half_n = n / 2;
        let second_n = n - half_n;

        // Each half may still exceed 65535 workgroups. Sub-chunk each half.
        let lo_chunks = chunk_ranges(half_n, MAX_WG);
        let hi_chunks = chunk_ranges(second_n, MAX_WG);

        // Staging for full readback.
        let y_size = (n * 4) as u64;
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lm_head_staging"),
            size: y_size,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("lm_head_encoder"),
        });

        let mut staging_offset: u64 = 0;

        // Dispatch lo chunks.
        for (start, count) in &lo_chunks {
            let chunk_bytes = (*count * 4) as u64;
            let y_chunk = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lm_head_y_chunk"),
                size: chunk_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            // The W buffer for this chunk starts at row `start` within lm_head_f32_lo.
            // But our shader always reads W from row 0 of the bound buffer. So we need
            // to bind a sub-range. wgpu doesn't support buffer sub-range bindings easily,
            // so instead we use a push constant `row_offset` trick — but our shader
            // doesn't have that field. Simplest: create a view buffer per chunk.
            //
            // Actually, the shader reads `w[row * k + col]` where `row` = workgroup_id.
            // If we bind the FULL lo buffer and set push.n = count, push.k = k, the
            // shader will read rows 0..count. We need rows start..start+count.
            // The shader doesn't have a row_offset. Let's add one to the push struct.
            //
            // WAIT — the push struct IS `{k, n, _pad0, _pad1}`. We can repurpose _pad0
            // as row_offset. But that changes the shader. Simpler: just bind a buffer
            // slice via offset. wgpu supports `BufferBinding { offset, size }`.
            let w_offset = (*start * k * 4) as u64;
            let w_size = (*count * k * 4) as u64;
            let push_buf = self.f32_matvec.make_push_buffer(k as u32, *count as u32);

            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lm_head_chunk_bg"),
                layout: &self.f32_matvec.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.lm_head_f32_lo,
                            offset: w_offset,
                            size: Some(std::num::NonZeroU64::new(w_size).unwrap()),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: x_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: y_chunk.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: push_buf.as_entire_binding() },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("lm_head_pass"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.f32_matvec.pipeline);
                pass.set_bind_group(0, Some(&bg), &[]);
                pass.dispatch_workgroups(*count as u32, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&y_chunk, 0, &staging, staging_offset, chunk_bytes);
            staging_offset += chunk_bytes;
        }

        // Dispatch hi chunks.
        for (start, count) in &hi_chunks {
            let chunk_bytes = (*count * 4) as u64;
            let y_chunk = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("lm_head_y_chunk_hi"),
                size: chunk_bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
                mapped_at_creation: false,
            });
            let w_offset = (*start * k * 4) as u64;
            let w_size = (*count * k * 4) as u64;
            let push_buf = self.f32_matvec.make_push_buffer(k as u32, *count as u32);

            let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("lm_head_chunk_bg_hi"),
                layout: &self.f32_matvec.bgl,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                            buffer: &self.lm_head_f32_hi,
                            offset: w_offset,
                            size: Some(std::num::NonZeroU64::new(w_size).unwrap()),
                        }),
                    },
                    wgpu::BindGroupEntry { binding: 1, resource: x_buf.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 2, resource: y_chunk.as_entire_binding() },
                    wgpu::BindGroupEntry { binding: 3, resource: push_buf.as_entire_binding() },
                ],
            });
            {
                let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                    label: Some("lm_head_pass_hi"),
                    timestamp_writes: None,
                });
                pass.set_pipeline(&self.f32_matvec.pipeline);
                pass.set_bind_group(0, Some(&bg), &[]);
                pass.dispatch_workgroups(*count as u32, 1, 1);
            }
            encoder.copy_buffer_to_buffer(&y_chunk, 0, &staging, staging_offset, chunk_bytes);
            staging_offset += chunk_bytes;
        }

        let sub_idx = queue.submit(std::iter::once(encoder.finish()));
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
        device.poll(wgpu::Maintain::WaitForSubmissionIndex(sub_idx));
        let _ = rx.recv();
        let mapped = slice.get_mapped_range();
        let logits: Vec<f32> = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        staging.unmap();
        logits
    }

    // ── End-to-end token generation ──────────────────────────────────────

    /// Greedy / top-p sampling loop over the prompt and `max_new_tokens`.
    pub fn generate(
        &self,
        prompt_tokens: &[u32],
        params: &SamplingParams,
    ) -> Result<Vec<u32>, String> {
        if prompt_tokens.is_empty() {
            return Err("empty prompt".into());
        }
        let mut kv_cache = KvCache::new(self.layers.len(), 4096);
        let mut output = Vec::<u32>::new();

        // 1. Prefill: walk the prompt to seed the KV cache.
        let mut last_logits = vec![0.0f32; self.vocab_size];
        for (pos, &tok) in prompt_tokens.iter().enumerate() {
            last_logits = self.forward_token(tok, pos, &mut kv_cache);
            kv_cache.advance();
        }

        // 2. Sample new tokens.
        let mut pos = prompt_tokens.len();
        for _ in 0..params.max_new_tokens {
            let next = sample_logits(&last_logits, params.temperature, params.top_p);
            if params.stop_tokens.contains(&next) {
                break;
            }
            output.push(next);
            last_logits = self.forward_token(next, pos, &mut kv_cache);
            kv_cache.advance();
            pos += 1;
        }
        Ok(output)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Config + metadata helpers
// ─────────────────────────────────────────────────────────────────────────────

impl Gemma4Config {
    fn num_layers(&self) -> usize {
        // This method is only called from Gemma4Runner::generate via
        // KvCache::new. The runner passes self.layers.len() directly now.
        // This stub exists only to satisfy the compiler; it should never
        // be called from outside the runner.
        panic!("Gemma4Config::num_layers() called directly — use runner.layers.len()")
    }
}

fn read_gemma4_config(weights: &ModelWeights) -> Result<Gemma4Config, String> {
    let hidden_size =
        get_u32_meta(weights, "gemma4.embedding_length").ok_or("missing embedding_length")? as usize;
    let num_heads = get_u32_meta(weights, "gemma4.attention.head_count")
        .ok_or("missing attention.head_count")? as usize;
    // num_kv_heads is per-layer; default to 8 here, used only as fallback.
    let num_kv_heads = 8;
    let head_dim_full = get_u32_meta(weights, "gemma4.attention.key_length").unwrap_or(512) as usize;
    let head_dim_swa =
        get_u32_meta(weights, "gemma4.attention.key_length_swa").unwrap_or(256) as usize;
    let intermediate_size_dense =
        get_u32_meta(weights, "gemma4.feed_forward_length").ok_or("missing feed_forward_length")? as usize;
    let rope_theta_full =
        get_f32_meta(weights, "gemma4.rope.freq_base").unwrap_or(1_000_000.0);
    let rope_theta_swa =
        get_f32_meta(weights, "gemma4.rope.freq_base_swa").unwrap_or(10_000.0);
    let sliding_window =
        get_u32_meta(weights, "gemma4.attention.sliding_window").unwrap_or(1024) as usize;
    let rms_norm_eps =
        get_f32_meta(weights, "gemma4.attention.layer_norm_rms_epsilon").unwrap_or(1e-6);

    Ok(Gemma4Config {
        hidden_size,
        num_heads,
        num_kv_heads,
        head_dim_full,
        head_dim_swa,
        intermediate_size_dense,
        rope_theta_full,
        rope_theta_swa,
        sliding_window,
        rms_norm_eps,
    })
}

fn read_moe_config(weights: &ModelWeights) -> Result<MoeFfnConfig, String> {
    let hidden = get_u32_meta(weights, "gemma4.embedding_length")
        .ok_or("missing embedding_length")? as usize;
    let expert_inner = get_u32_meta(weights, "gemma4.expert_feed_forward_length")
        .ok_or("missing expert_feed_forward_length")? as usize;
    let n_experts =
        get_u32_meta(weights, "gemma4.expert_count").ok_or("missing expert_count")? as usize;
    let top_k =
        get_u32_meta(weights, "gemma4.expert_used_count").unwrap_or(8) as usize;
    Ok(MoeFfnConfig {
        hidden,
        expert_inner,
        n_experts,
        top_k,
    })
}

fn read_head_count_kv_array(
    weights: &ModelWeights,
    n_layers: usize,
) -> Result<Vec<usize>, String> {
    if let Some(GgufValue::Array(arr)) = weights.metadata.get("gemma4.attention.head_count_kv") {
        let mut out = Vec::with_capacity(n_layers);
        for (i, v) in arr.iter().enumerate() {
            let n = match v {
                GgufValue::U32(x) => *x as usize,
                GgufValue::I32(x) => *x as usize,
                GgufValue::U64(x) => *x as usize,
                _ => return Err(format!("head_count_kv[{i}] has unexpected type")),
            };
            out.push(n);
        }
        if out.len() != n_layers {
            return Err(format!(
                "head_count_kv array length {} != n_layers {}",
                out.len(),
                n_layers
            ));
        }
        Ok(out)
    } else {
        // Fallback: scalar value broadcast.
        let scalar = get_u32_meta(weights, "gemma4.attention.head_count_kv")
            .ok_or("missing attention.head_count_kv")? as usize;
        Ok(vec![scalar; n_layers])
    }
}

fn read_swa_pattern(weights: &ModelWeights, n_layers: usize) -> Result<Vec<bool>, String> {
    if let Some(GgufValue::Array(arr)) =
        weights.metadata.get("gemma4.attention.sliding_window_pattern")
    {
        let mut out = Vec::with_capacity(n_layers);
        for v in arr {
            let b = match v {
                GgufValue::Bool(b) => *b,
                GgufValue::U32(x) => *x != 0,
                GgufValue::I32(x) => *x != 0,
                _ => return Err("sliding_window_pattern entry has unexpected type".into()),
            };
            out.push(b);
        }
        if out.len() != n_layers {
            return Err(format!(
                "sliding_window_pattern length {} != n_layers {}",
                out.len(),
                n_layers
            ));
        }
        Ok(out)
    } else {
        // No SWA pattern → assume all layers are full attention.
        Ok(vec![false; n_layers])
    }
}

fn read_vocab_size(weights: &ModelWeights) -> Result<usize, String> {
    Ok(weights.vocab_size)
}

fn get_u32_meta(weights: &ModelWeights, key: &str) -> Option<u32> {
    match weights.metadata.get(key)? {
        GgufValue::U32(x) => Some(*x),
        GgufValue::I32(x) => Some(*x as u32),
        GgufValue::U64(x) => Some(*x as u32),
        _ => None,
    }
}

fn get_f32_meta(weights: &ModelWeights, key: &str) -> Option<f32> {
    match weights.metadata.get(key)? {
        GgufValue::F32(x) => Some(*x),
        GgufValue::U32(x) => Some(*x as f32),
        _ => None,
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Tensor loaders
// ─────────────────────────────────────────────────────────────────────────────

fn load_dequantized_or_fail(
    weights: &ModelWeights,
    name: &str,
    expected: usize,
) -> Result<Vec<f32>, String> {
    let region = weights
        .tensors
        .get(name)
        .ok_or_else(|| format!("missing tensor `{}`", name))?;
    let bytes = weights
        .tensor_bytes(name)
        .ok_or_else(|| format!("tensor `{}` has no mapped bytes", name))?;
    let n_elements: usize = region.shape.iter().product();
    let mut out = bridge::dequantize_tensor(bytes, region.quant_type, n_elements);
    if out.len() < expected {
        return Err(format!(
            "tensor `{}` produced {} elements, expected {}",
            name, out.len(), expected
        ));
    }
    out.truncate(expected);
    Ok(out)
}

fn load_norm_for_layer(
    weights: &ModelWeights,
    layer_idx: usize,
    suffix: &str,
    expected: usize,
) -> Result<Vec<f32>, String> {
    load_dequantized_or_fail(
        weights,
        &format!("blk.{}.{}.weight", layer_idx, suffix),
        expected,
    )
}

fn upload_raw_quant(
    weights: &ModelWeights,
    name: &str,
    expected_qtype: u32,
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    label: &'static str,
) -> Result<wgpu::Buffer, String> {
    let region = weights
        .tensors
        .get(name)
        .ok_or_else(|| format!("missing tensor `{}`", name))?;
    if region.quant_type != expected_qtype {
        return Err(format!(
            "tensor `{}`: expected qtype {} got {}",
            name, expected_qtype, region.quant_type
        ));
    }
    let bytes = weights
        .tensor_bytes(name)
        .ok_or_else(|| format!("tensor `{}` has no mapped bytes", name))?;

    let padded = (bytes.len() + 3) & !3;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: padded as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, bytes);
    if padded > bytes.len() {
        let pad = vec![0u8; padded - bytes.len()];
        queue.write_buffer(&buf, bytes.len() as u64, &pad);
    }
    Ok(buf)
}

fn upload_fp32_tensor(
    weights: &ModelWeights,
    name: &str,
    expected: usize,
    device: &Arc<wgpu::Device>,
    queue: &Arc<wgpu::Queue>,
    label: &'static str,
) -> Result<wgpu::Buffer, String> {
    let data = load_dequantized_or_fail(weights, name, expected)?;
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (data.len() * 4) as u64,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&buf, 0, bytemuck::cast_slice(&data));
    Ok(buf)
}

// ─────────────────────────────────────────────────────────────────────────────
// CPU helpers (RMSNorm + sampling)
// ─────────────────────────────────────────────────────────────────────────────

fn rmsnorm_gemma_inplace(x: &mut [f32], weight: &[f32], eps: f32) {
    let n = x.len();
    debug_assert_eq!(weight.len(), n);
    let mut sum_sq = 0.0f32;
    for &xi in x.iter() {
        sum_sq += xi * xi;
    }
    let inv_rms = 1.0 / ((sum_sq / n as f32) + eps).sqrt();
    let plus_one = std::env::var("GEMMA4_NORM_PLUS_ONE").map(|v| v == "1").unwrap_or(false);
    if plus_one {
        for i in 0..n {
            x[i] = x[i] * inv_rms * (weight[i] + 1.0);
        }
    } else {
        for i in 0..n {
            x[i] = x[i] * inv_rms * weight[i];
        }
    }
}

/// Greedy / temperature / top-p sampling matching `generate.rs::sample_next_token`.
fn sample_logits(logits: &[f32], temperature: f32, top_p: f32) -> u32 {
    if temperature < 0.01 {
        return logits
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(idx, _)| idx as u32)
            .unwrap_or(0);
    }
    let max_logit = logits
        .iter()
        .filter(|x| x.is_finite())
        .fold(f32::NEG_INFINITY, |a, &b| a.max(b));
    if !max_logit.is_finite() {
        return 0;
    }
    let mut probs: Vec<(usize, f32)> = logits
        .iter()
        .enumerate()
        .map(|(idx, &l)| {
            if !l.is_finite() {
                (idx, 0.0)
            } else {
                (idx, ((l - max_logit) / temperature).exp())
            }
        })
        .collect();
    let sum: f32 = probs.iter().map(|(_, p)| p).sum();
    if sum <= 0.0 {
        return 0;
    }
    for entry in probs.iter_mut() {
        entry.1 /= sum;
    }
    probs.sort_unstable_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut cumulative = 0.0f32;
    let mut cutoff = probs.len();
    for (i, (_, p)) in probs.iter().enumerate() {
        cumulative += p;
        if cumulative >= top_p {
            cutoff = i + 1;
            break;
        }
    }
    probs.truncate(cutoff);
    let final_sum: f32 = probs.iter().map(|(_, p)| p).sum();
    let r = rand::random::<f32>() * final_sum;
    let mut acc = 0.0f32;
    for (id, p) in &probs {
        acc += p;
        if r <= acc {
            return *id as u32;
        }
    }
    probs.last().map(|(id, _)| *id as u32).unwrap_or(0)
}

// ─────────────────────────────────────────────────────────────────────────────
// Device init
// ─────────────────────────────────────────────────────────────────────────────

fn init_device(gpu_index: usize) -> Result<(Arc<wgpu::Device>, Arc<wgpu::Queue>), String> {
    let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
        backends: wgpu::Backends::VULKAN,
        ..Default::default()
    });
    let adapters: Vec<_> = instance
        .enumerate_adapters(wgpu::Backends::VULKAN)
        .into_iter()
        .filter(|a| matches!(a.get_info().device_type, wgpu::DeviceType::DiscreteGpu))
        .collect();
    if adapters.is_empty() {
        return Err("no discrete Vulkan GPUs visible".into());
    }
    let adapter = &adapters[gpu_index.min(adapters.len() - 1)];
    let info = adapter.get_info();
    tracing::info!("Selected GPU {}: {} ({:?})", gpu_index, info.name, info.backend);

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: Some("gemma4_runner"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits {
                max_storage_buffer_binding_size: 2 * 1024 * 1024 * 1024 - 1, // 2 GB (P100 max)
                max_buffer_size: u64::MAX,
                ..Default::default()
            },
            memory_hints: wgpu::MemoryHints::Performance,
        },
        None,
    ))
    .map_err(|e| format!("request_device: {e}"))?;

    Ok((Arc::new(device), Arc::new(queue)))
}

// Silence dead_code on the embedded MatvecPush we re-export pattern;
// remove if a future PR uses it directly.
#[allow(dead_code)]
fn _suppress_unused_matvec_push(_p: MatvecPush) {}

/// Split `total` into chunks of at most `max_per_chunk`, returning
/// `(start_row, count)` pairs.
fn chunk_ranges(total: usize, max_per_chunk: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut offset = 0;
    while offset < total {
        let count = (total - offset).min(max_per_chunk);
        out.push((offset, count));
        offset += count;
    }
    out
}
