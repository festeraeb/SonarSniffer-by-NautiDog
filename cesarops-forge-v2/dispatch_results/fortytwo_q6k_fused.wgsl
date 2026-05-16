 The shader must compile and run correctly on the target GPU.

```wgsl
struct Params {
  N: u32,
  K: u32,
}

@compute @workgroup_size(256)
fn matvec_q6k_fused(@builtin(global_invocation_id) global_id: vec3<u32>) -> void {
  let tid = global_id.x;
  if tid >= params.N { return; }

  let mut acc = f32(0.0);
  for k_block in 0u .. params.K / 256u {
    let block_offset = k_block * 210u;
    let ql_ptr = &q6k_blocks[block_offset];
    let qh_ptr = &q6k_blocks[block_offset + 128u];
    let scales_ptr = &q6k_blocks[block_offset + 192u];
    let d_bytes = &q6k_blocks[block_offset + 208u] as ptr<u16>;
    let d = bitcast<f16>(d_bytes);

    for l in 0u .. 32u {
      let is = l * 2u;
      let scale_l = bitcast<f32>(scales_ptr[is]);
      let scale_l2 = bitcast<f32>(scales_ptr[is + 1]);
      let scale_l3 = bitcast<f32>(scales_ptr[is + 2]);
      let scale_l4 = bitcast<f32>(scales_ptr[is + 3]);

      let ql_l = ql_ptr[l];
      let ql_l2 = ql_ptr[l + 32u];
      let ql_l3 = ql_ptr[l + 64u];
      let ql_l4 = ql_ptr[l + 96u];

      let qh_l = qh_ptr[l];
      let qh_l2 = qh_ptr[l + 32u];
      let qh_l3 = qh_ptr[l + 64u];
      let qh_l4 = qh_ptr[l + 96u];

      acc += (f32(ql_l) + f32(qh_l) * d) * scale_l * input[tid * params.K + k_block * 256u + l];
      acc += (f32(ql_l2) + f32(qh_l2) * d) * scale_l2 * input[tid * params.K + k_block * 256u + l + 32u];
      acc += (f32(ql_l3) + f32(qh_l3) * d) * scale_l3 * input[tid * params.K + k_block * 256u + l + 64u];
      acc += (f32(ql_l4) + f32(qh_l4) * d) * scale_l4 * input[tid * params.K + k_block * 256u + l + 96u];
    }
  }

  output[tid] = acc;
}
```
