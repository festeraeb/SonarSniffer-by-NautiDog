# MoE Production Extension — External Contribution

Source: dropped in by operator from a friend, 2026-05-16.
Status: **REFERENCE MATERIAL — NOT INTEGRATED YET.**

This is a complete production-grade MoE design (gating shader, coalesced expert
MLP shader, down-projection shader, host dispatcher, GGUF loader hooks) targeted
at our wgpu/Vulkan engine on dual P100. The strategy is:

1. Top-2 routing via softmax in a small gate shader
2. Token coalescing — group tokens by assigned expert before dispatch
3. Single unified expert MLP kernel where every workgroup processes one expert
   exclusively (no warp divergence on Pascal)
4. Down-projection with routing-coefficient scaling and accumulation back into
   the residual stream

Why we're staging it:
- Our `moe.rs` today is f64 router scaffolding with TODO `dispatch_experts`
  stubs — there's no production MoE forward pass.
- This drop fills that gap with a real design plus shaders.
- Integration touches: `arch_detect`, `forward_pass`, `transformer`,
  `tensor_loader_safe`, `pipeline_init`, plus three new WGSL files.
- That's a substantial spec on its own. Queueing it after multi-model
  loading (the spec the operator is working on now).

When we pick this up, we should:
- Validate the shaders against naga (the contributed code uses
  `dot(vec4, vec4)` and standard wgpu patterns — should compile).
- Replace the host-side token sort (currently `read_buffer_to_cpu`) with
  a GPU prefix-sum scan once we're past initial bring-up. The CPU bounce
  is the explicit "T440 build parser" fallback per the contribution.
- Cross-validate top-2 routing against a small known-good MoE (Gemma-4-MoE
  test prompts, since that's already on the fleet).
- Pair with the FLOPS instrumentation work (`--bench` mode) so we can
  measure expert-utilization and verify we're actually saturating both
  P100s.

Key architectural pieces below — copied verbatim so we have a frozen
reference even if the upstream changes.

---

## Memory Layout

```rust
pub struct MoEConfig {
    pub num_experts: u32,
    pub num_experts_per_token: u32, // Top-k (typically 2)
    pub expert_intermediate_dim: u32,
}

pub struct MoELayerWeights {
    pub gate_inplace_weights: wgpu::Buffer,  // [hidden_dim, num_experts]
    pub expert_gate_proj: wgpu::Buffer,       // [num_experts, intermediate_dim, hidden_dim]
    pub expert_up_proj: wgpu::Buffer,
    pub expert_down_proj: wgpu::Buffer,
}

pub struct MoEExecutionBatch {
    pub token_reindexing_map: wgpu::Buffer,  // global flat -> original token pos
    pub expert_token_counts: wgpu::Buffer,   // tokens per expert
    pub expert_offsets: wgpu::Buffer,        // start positions
    pub routing_weights: wgpu::Buffer,       // [batch, num_experts_per_token]
}
```

---

## moe_gate.wgsl — Top-2 Routing

```wgsl
struct GatingParams {
    batch_size: u32,
    num_experts: u32,
    hidden_dim: u32,
    pad: u32,
};

@group(0) @binding(0) var<uniform> params: GatingParams;
@group(0) @binding(1) var<storage, read> token_hidden_states: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> gate_weights: array<vec4<f32>>;
@group(0) @binding(3) var<storage, read_write> topk_indices: array<vec2<u32>>;
@group(0) @binding(4) var<storage, read_write> topk_scales: array<vec2<f32>>;

@compute @workgroup_size(64, 1, 1)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let token_idx = global_id.x;
    if (token_idx >= params.batch_size) { return; }

    let vecs_per_hidden = params.hidden_dim / 4u;
    let token_offset = token_idx * vecs_per_hidden;

    var max_val_1 = -1e30;
    var max_val_2 = -1e30;
    var max_idx_1 = 0u;
    var max_idx_2 = 0u;

    for (var e = 0u; e < params.num_experts; e = e + 1u) {
        let expert_offset = e * vecs_per_hidden;
        var score: f32 = 0.0;
        for (var i = 0u; i < vecs_per_hidden; i = i + 1u) {
            let t_val = token_hidden_states[token_offset + i];
            let w_val = gate_weights[expert_offset + i];
            score = score + dot(t_val, w_val);
        }
        if (score > max_val_1) {
            max_val_2 = max_val_1;
            max_idx_2 = max_idx_1;
            max_val_1 = score;
            max_idx_1 = e;
        } else if (score > max_val_2) {
            max_val_2 = score;
            max_idx_2 = e;
        }
    }

    let exp1 = exp(max_val_1 - max_val_1); // 1.0
    let exp2 = exp(max_val_2 - max_val_1);
    let sum_exp = exp1 + exp2;

    topk_indices[token_idx] = vec2<u32>(max_idx_1, max_idx_2);
    topk_scales[token_idx] = vec2<f32>(exp1 / sum_exp, exp2 / sum_exp);
}
```

---

## moe_expert_mlp.wgsl — Coalesced Expert Parallel SwiGLU

```wgsl
struct ExpertParams {
    hidden_dim: u32,
    intermediate_dim: u32,
    total_tokens: u32,
    num_experts: u32,
};

@group(0) @binding(0) var<uniform> params: ExpertParams;
@group(0) @binding(1) var<storage, read> coalesced_hidden_states: array<vec4<f32>>;
@group(0) @binding(2) var<storage, read> expert_offsets: array<u32>;
@group(0) @binding(3) var<storage, read> expert_gate_proj: array<vec4<f32>>;
@group(0) @binding(4) var<storage, read> expert_up_proj: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read_write> intermediate_outputs: array<vec4<f32>>;

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>
) {
    let expert_idx = workgroup_id.z;
    let token_expert_offset = global_id.x;
    let inter_dim_idx = global_id.y;

    let start_token = expert_offsets[expert_idx];
    let end_token = expert_offsets[expert_idx + 1u];
    let active_tokens_for_expert = end_token - start_token;

    if (token_expert_offset >= active_tokens_for_expert || inter_dim_idx >= params.intermediate_dim) {
        return;
    }

    let global_token_idx = start_token + token_expert_offset;
    let vecs_per_hidden = params.hidden_dim / 4u;

    let weight_expert_stride = expert_idx * params.intermediate_dim * vecs_per_hidden;
    let weight_row_offset = weight_expert_stride + (inter_dim_idx * vecs_per_hidden);
    let input_token_offset = global_token_idx * vecs_per_hidden;

    var gate_accum: f32 = 0.0;
    var up_accum: f32 = 0.0;

    for (var i = 0u; i < vecs_per_hidden; i = i + 1u) {
        let x = coalesced_hidden_states[input_token_offset + i];
        let w_gate = expert_gate_proj[weight_row_offset + i];
        let w_up = expert_up_proj[weight_row_offset + i];
        gate_accum = gate_accum + dot(x, w_gate);
        up_accum = up_accum + dot(x, w_up);
    }

    let silu_gate = gate_accum * (1.0 / (1.0 + exp(-gate_accum)));
    let final_value = silu_gate * up_accum;

    let out_stride = params.intermediate_dim;
    let out_idx = (global_token_idx * out_stride) + inter_dim_idx;
    // NOTE: contribution did not write the final scalar back to intermediate_outputs.
    // Need to add: intermediate_outputs[(out_idx) / 4] = ... with proper vec4 packing,
    // OR reshape the binding to array<f32> and write scalar. Polish during integration.
}
```

---

## moe_down_proj.wgsl — Down-Projection + Re-composition

```wgsl
struct DownProjParams {
    hidden_dim: u32,
    intermediate_dim: u32,
    batch_size: u32,
    num_experts: u32,
};

@group(0) @binding(0) var<uniform> params: DownProjParams;
@group(0) @binding(1) var<storage, read> intermediate_outputs: array<f32>;
@group(0) @binding(2) var<storage, read> coalesced_token_map: array<u32>;
@group(0) @binding(3) var<storage, read> expert_offsets: array<u32>;
@group(0) @binding(4) var<storage, read> expert_down_proj: array<vec4<f32>>;
@group(0) @binding(5) var<storage, read> topk_indices: array<vec2<u32>>;
@group(0) @binding(6) var<storage, read> topk_scales: array<vec2<f32>>;
@group(0) @binding(7) var<storage, read_write> output_hidden_states: array<vec4<f32>>;

@compute @workgroup_size(16, 16, 1)
fn main(
    @builtin(global_invocation_id) global_id: vec3<u32>,
    @builtin(workgroup_id) workgroup_id: vec3<u32>
) {
    let expert_idx = workgroup_id.z;
    let token_expert_offset = global_id.x;
    let hidden_vec_idx = global_id.y;

    let start_token = expert_offsets[expert_idx];
    let end_token = expert_offsets[expert_idx + 1u];
    let active_tokens_for_expert = end_token - start_token;

    let vecs_per_hidden = params.hidden_dim / 4u;

    if (token_expert_offset >= active_tokens_for_expert || hidden_vec_idx >= vecs_per_hidden) {
        return;
    }

    let global_coalesced_idx = start_token + token_expert_offset;
    let original_token_idx = coalesced_token_map[global_coalesced_idx];

    let indices = topk_indices[original_token_idx];
    let scales = topk_scales[original_token_idx];

    var routing_scale: f32 = 0.0;
    if (indices.x == expert_idx) { routing_scale = scales.x; }
    else if (indices.y == expert_idx) { routing_scale = scales.y; }
    if (routing_scale <= 0.0) { return; }

    let weight_expert_stride = expert_idx * vecs_per_hidden * params.intermediate_dim;
    let weight_row_offset = weight_expert_stride + (hidden_vec_idx * params.intermediate_dim);
    let input_inter_offset = global_coalesced_idx * params.intermediate_dim;

    var accumulated_vec = vec4<f32>(0.0, 0.0, 0.0, 0.0);
    for (var i = 0u; i < params.intermediate_dim; i = i + 1u) {
        let intermediate_scalar = intermediate_outputs[input_inter_offset + i];
        let weight_vec = expert_down_proj[weight_row_offset + i];
        accumulated_vec = accumulated_vec + (weight_vec * intermediate_scalar);
    }

    let final_scaled_vec = accumulated_vec * routing_scale;
    let target_out_idx = (original_token_idx * vecs_per_hidden) + hidden_vec_idx;
    output_hidden_states[target_out_idx] = output_hidden_states[target_out_idx] + final_scaled_vec;
    // NOTE: this is a non-atomic read-modify-write. Across the two top-k experts
    // for the same token, there's a race. Production needs either atomic adds
    // or the per-expert outputs written to disjoint scratch buffers and a
    // second-pass reduction. Polish during integration.
}
```

---

## Host: MoE Dispatcher (Rust)

[Full source archived in this file's git history; key call shape:]

```rust
pub fn dispatch_moe_layer(
    &self,
    encoder: &mut wgpu::CommandEncoder,
    config: &ModelConfig,
    moe_config: &MoEConfig,
    weights: &MoELayerWeights,
    input_hidden_states: &wgpu::Buffer,
    batch_size: u32,
) -> wgpu::Buffer {
    // 1. Gate shader -> top-2 indices + scales
    // 2. CPU readback (BLOCKING) to compute coalesced token map + offsets
    //    [TODO: replace with GPU prefix-sum scan after bring-up]
    // 3. Upload coalesced_map + offsets back to GPU
    // 4. Expert MLP shader (single dispatch, num_experts workgroups in Z)
    // 5. Returns intermediate_output_buf for the down-projection pass
}

pub fn dispatch_down_projection(...) -> wgpu::Buffer {
    // Down-proj + scale + accumulate into output_hidden_states
}
```

GGUF loader hooks expect tensor names of the form:
- `blk.{layer}.ffn_gate_inp.weight` — the gate matrix [hidden_dim × num_experts]
- `blk.{layer}.ffn_gate.weight.0` — packed expert gate proj
- `blk.{layer}.ffn_up.weight.0` — packed expert up proj
- `blk.{layer}.ffn_down.weight.0` — packed expert down proj

---

## Polish notes for integration

1. **moe_expert_mlp.wgsl is missing the final write.** The contribution
   computes `final_value` but doesn't store it. Integration pass must
   either pack 4 scalar outputs into a vec4 write, or reshape binding 5
   to `array<f32>` and do a scalar write.

2. **Race in moe_down_proj.wgsl.** Two top-k experts targeting the same
   `original_token_idx` will race on `output_hidden_states[target_out_idx]`.
   Resolution options:
   - Atomic add (wgpu doesn't expose `atomicAdd` for f32)
   - Write each expert to a disjoint scratch slot, second-pass reduce
   - Sequence the two top-k passes (slower but correct)

3. **CPU readback in `dispatch_moe_layer`** is a hard sync point on the
   GPU pipeline. Will pause the queue every layer. Acceptable for v1
   bring-up; replace with on-GPU prefix-sum scan post-bring-up.

4. **MoE config parsing in `moe_initializer`** uses `metadata.get(...).parse()`.
   Our `loader::GgufValue` is an enum, not stringly-typed; the helper needs
   adapting to use `as_u32` walks like `arch_detect.rs` does.

5. **Tensor names**: GGUF MoE files vary. Some use `.weight` array indexed
   by expert (`.0`, `.1`, ...), some use `expert_gate.{e}.weight`. Loader
   needs to detect both patterns.

6. **Test target on the fleet:** Gemma-4-26B-MoE-IQ4_XS (already on P100 #0
   via koboldcpp at port 5001, also locally on cesarops2 at /mnt/storage).
   We have a working koboldcpp baseline for output comparison.
