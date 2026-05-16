</think>

## Recommendation

### Shader interface
```glslsl
uniform float pos;
uniform float head_dim;
uniform int kv_stride;
uniform int kv_head_offset;
uniform float theta;
uniform int rope_mode; // 0: none, 1: q, k only

matvec_qkv_rope_pc gen_qkv_rope_qkv_cache_write(float q, float k, float v, float q_proj, float k_proj, float v_proj, float q_bias, float k_bias, float v_bias) {
    float q_out[4], k_out[4], v_out[4];
    float q_rot[4], k_rot[4];

    // Apply RoPE to Q and K
    if (rope_mode == 1) {
        // Apply RoPE to Q and K
        for (int j = 0; j < head_dim; j++) {
            float angle = pos / pow(theta, 2.0 * j / head_dim);
            float sin_angle = sin(angle);
            float cos_angle = cos(angle);
            
            float q_rot[j] = q[j] * cos_angle - q[j + head_dim / 2] * sin_angle;
            float k_rot[j] = k[j] * cos_angle - k[j + head_dim / 2] * sin_angle;
        }
        
        q_proj[0] = q_rot[0];
        q_proj[1] = q_rot[1];
        q_proj[2] = q_rot[2];
        q_proj[3] = q_rot[3];
        
        k_proj[0] = k_rot[0];
        k_proj[1] = k_rot[1];
        k_proj[2] = k_rot[2];
        k_proj[3] = k_rot[3];
    }

    // Apply projection matrices
    matvec_qkv_pc gen_qkv_proj_qkv_cache_write(float q_in[4], float k_in[4], float v_in[4], float q_proj[4][4], float k_proj[4][4], float v_proj[4][4]) {
        // Q projection
        float q_out[4];
        for (int i = 0; i < 4; i++) {
            q_out[i] = q_proj[i][0] * q_in[0] + q_proj[i][1] * q_in[1] + q_proj[i][2] * q_in[2] + q_proj[i][3] * q_in[3];
        }
        q_out[0] += q_bias[0];
        q_out[1] += q_bias[1];
        q_out[2] += q_bias[2];
        q_out[3] += q_bias[3];
        
        // K projection
        float k_out[4];
        for (int i = 0; i < 4; i++) {
            k_out[i] = k_proj[i][0] * k_in[0] + k_proj[i][1] * k_in[1] + k_proj[i][2] * k_in[2] + k_proj[i][3] * k_in[3];
        }
        k_out[0] += k_bias[0];
        k_out[1] += k_bias[1];
        k_out[2] += k_bias[2];
        k_out[3] += k_bias[3];
        
        // V projection
        float v_out[4];
        for (int i = 0; i < 4; i++) {
            v_out[i] = v_proj[i][0] * v_in[0] + v_proj[i][1] * v_in[1] + v_proj[i][2] * v_in[2] + v_proj[i][3] * v_in[3];
        }
        
        // Write Q, K, V to cache
        float q_cache[4];
        float k_cache[4];
        float v_cache[4];
        
        // Write Q to cache
        int q_head = kv_head_offset;
        float q_pos = pos * kv_stride;
        float q_dim = head_dim;
        float q_offset = q_head * kv_stride * 4 + q_head * head_dim * 4 + q_dim * 4;
        float q_value = q_out[q_dim];
        float q_write = q_value + q_offset;
        float q_write_float = (float) q_write;
        float q_write_int = (int) q_write_float;
        float q_write_frac = q_write_float - q_write_int;
        float q_write_color = vec4(q_write_int, q_write_int, q_write_int, 1.0);
        float q_write_alpha = q_write_frac * 255.0;
        float q_write_alpha_clamped = clampf(q_write_alpha, 0.0, 1.0);
        float q_write_color_alpha = vec4(q_write_color[0], q_write_color[1], q_write_color[2], q_write_alpha_clamped);
        float q_write_color_alpha_float = float(q_write_color_alpha);
        float q_write_color_alpha_int = (int) q_write_color_alpha_float;
        float q_write_color_alpha_frac = q_write_color_alpha_float - q_write_color_alpha_int;
        float q_write_color_alpha_alpha = q_write_color_alpha_frac * 255.0;
        float q_write_color_alpha_alpha_clamped = clampf(q_write_color_alpha_alpha, 0.0, 1.0);
        float q_write_color_alpha_alpha_clamped_float = float(q_write_color_alpha_alpha_clamped);
        float q_write_color_alpha_alpha_clamped_int = (int) q_write_color_alpha_alpha_clamped_float;
        float q_write_color_alpha_alpha_clamped_frac = q_write_color_alpha_alpha_clamped_float - q_write_color_alpha_alpha_clamped_int;
        float q_write_color_alpha_alpha_clamped_frac_clamped = clampf(q_write_color_alpha_alpha_clamped_frac, 0.0, 1.0);
        
        // Write K to cache
        int k_head = kv_head_offset + head_dim / 2;
        float k_pos = pos * kv_stride;
        float k_dim = head_dim;
        float k_offset = k_head * kv_stride * 4 + k_head * head_dim * 4 + k_dim * 4;
        float k_value = k_out[k_dim];
        float k_write = k_value + k_offset;
        float k_write_float = (float) k_write;
        float k_write_int = (int) k_write_float;
        float k_write_frac = k_write_float - k_write_int;
        float k_write_color = vec4(k_write_int, k_write_int, k_write_int, 1.0);
        float k_write_alpha = k_write_frac * 255.0;
        float k_write_alpha_clamped = clampf(k_write_alpha, 0.0, 1.0);
        float k_write_color_alpha = vec4(k_write_color[0], k_write_color[1], k_write_color[2], k_write_alpha_clamped);
        float k_write_color_alpha_float = float(k_write_color_alpha);
        float k_write_color_alpha_int = (int) k_write_color_alpha_float;
        float k_write_color_alpha_frac = k_write_color_alpha_float - k_write_color_alpha_int;
        float k_write_color_alpha_alpha = k_write_color_alpha_frac * 255.0;
        float k_write_color_alpha_alpha_clamped = clampf(k_write_color_alpha_alpha, 0.0, 1.0);
        float k_write_color_alpha_alpha_clamped_float = float(k_write_color_alpha_alpha_clamped);
        float k_write_color_alpha_alpha_clamped_int = (int) k_write_color_alpha_alpha_clamped_float;
        float k_write_color_alpha_alpha_clamped_frac = k_write_color_alpha_alpha_clamped_float - k_write_color_alpha_alpha_clamped_int;
        float k_write_color_alpha_alpha_clamped_frac_clamped = clampf(k_write_color_alpha_alpha_clamped_frac, 0.0, 1.0);
        
        // Write V to cache
        int v_head = kv_head_offset + head_dim;
        float v_pos = pos * kv_stride;
        float v_dim = head_dim;
        float v_offset = v_head * kv_stride * 4 + v_head * head_dim * 4 + v_dim * 4;
        float v_value = v_out[v_dim];
        float v_write = v_value + v_offset;
        float v_write_float = (float) v_write;
        float v_write_int = (int) v_write_float;
        float v_write_frac = v_write_float - v_write_int;
        float v_write_color = vec4(v_write_int, v_write_int, v_write_int, 1.0);
        float v_write_alpha = v_write_frac * 255.0;
        float v_write_alpha_clamped = clampf(v_write_alpha, 0.0, 1.0);
        float v_write_color_alpha = vec4(v_write_color[0], v_write_color[1], v_write_color[2], v_write_alpha_clamped);
        float v_write_color_alpha_float = float(v_write_color_alpha);
        float v_write_color_alpha_int = (int) v_write_color_alpha_float;
        float v_write_color_alpha_frac = v_write_color_alpha_float - v_write_color_alpha_int;
        float v_write_color_alpha_alpha = v_write_color_alpha_frac * 255.0;
        float v_write_color_alpha_alpha_clamped = clampf(v_write_color_alpha_alpha, 0.0, 1.0);
        float v_write_color_alpha_alpha_clamped_float = float(v_write_color_alpha_alpha_clamped);
        float v_write_color_alpha_alpha_clamped_int = (int) v_write_color_alpha_alpha_clamped_float;
        float v_write_color_alpha_alpha_clamped_frac = v_write_color_alpha_alpha_clamped_float - v_write_color_alpha_alpha_clamped_int;
        float v_write_color_alpha_alpha_clamped_frac_clamped = clampf(v_write_color_alpha_alpha_clamped_frac, 0.0, 1.0);
        
        return q_out, k_out, v_out;
    }
}
```

### Dispatch flow
```glslsl
// Launch gen_qkv_proj_qkv_cache_write with pos, head_dim, kv_stride, kv_head_offset, theta, rope_mode
gen_qkv_proj_qkv_cache_write gen_qkv_proj_qkv_cache_write(pos, head_dim, kv_stride, kv_head_offset, theta, rope_mode) {
    // Launch gen_qkv_proj_qkv_cache_write with q_in, k_in, v_in
    gen_qkv_proj_qkv_cache_write(q_in, k_in, v_in);
}
```

### Verification plan
To verify the correctness of the fused QKV+bias+RoPE+cache-write kernel, you can perform the following steps:
1. Use the existing diagnostic readback at pos=1 head=0 to compare the scores before and after the fusion.
2. Ensure that the parity of the scores matches, as the fused kernel should not change the correctness of the computation.

### Risks / Rollback
If the fused kernel introduces any regressions, you can rollback by reverting the changes made to the shader interface and dispatch flow. This includes removing the new `rope_mode` parameter from the shader interface and modifying the `matvec_qkv_proj_qkv_cache_write` function to no longer apply RoPE to Q and K.

```glslsl
## Recommendation

### Shader interface
```glslsl
uniform float pos;
uniform float head_dim;
uniform int kv_stride;
uniform int kv_head_offset;
uniform float theta;
uniform int rope_mode; // 0: none, 1: q, k only

matvec_qkv_rope_pc gen_qkv_rope_qkv_cache_write(float q, float k, float v, float q_proj, float k_proj, float v_proj, float q_bias, float k_bias, float v_bias) {
    float q_out[4], k_out[4], v_out[4];
    float q_rot[4], k_rot[4];

    // Apply RoPE to Q and K
    if (rope_mode == 1) {
        // Apply RoPE to Q and K
        for (int j = 0; j < head_dim; j++) {
            float angle = pos / pow(theta, 2.0 * j / head_dim);
            float sin_angle = sin(angle);
            float cos_angle = cos(angle);
            
            float q_rot[j] = q[j] * cos_angle - q[j + head_dim / 2] * sin_angle;
            float k_rot[j] = k[j] * cos_angle - k[j + head_dim / 2] * sin_angle;
        }
        
        q_proj[0] = q_rot[0];
        q_proj[1] = q_rot[1];
        q_proj[2] = q_rot[2];
        q_proj[3] = q_rot[3];
        
        k_proj[0] = k_rot[0];
        k_proj[1] = k_rot[1];
        k_proj[2] = k_rot[2];
        k_proj[3] = k_rot[3];
    }

    // Apply projection matrices
    matvec_qkv_pc gen_qkv_proj_qkv_cache_write(float q_in[4], float k_in[4], float v_in[4], float q_proj[4][4], float k_proj[4][4], float v_proj[4][4]) {
        // Q projection
        float q_out[4];
        for (int i = 0; i < 4; i++) {
            q_out[i] = q_proj[i][0] * q_in[0] + q_proj[i][1] * q_in[1] + q_proj[i][2] * q_in[2] + q_proj[i][3] * q_in[3];
        }
        q_out[0] += q_bias[0];
        q_out[1] += q_bias[1];
        q_out[2] += q_bias[2];
        q_out[3] += q_bias[3];
        
        // K projection
        float k_out[4];
        for (int i = 0; i < 4; i++) {
            k_out[i] = k_proj[i][0] * k_in[0] + k_proj[i][1] * k_in[1] + k_proj[i][2] * k_in[2] + k_proj[i][3] * k_in[3];
        }
        k_out[0] += k_bias[0];
        k_out[1] += k_bias[1];
        k_out[2] += k_bias[2];
        k_out[3] += k_bias[3];
        
        // V projection
        float v_out[4];
        for (int i = 0; i < 4; i++) {
            v_out[i] = v_proj[i][0] * v_in[0] + v_proj[i][1] * v_in[1] + v_proj[i][2] * v_in[2] + v_proj[i][3] * v_in[3];
        }
        
        // Write Q, K, V to cache
        float q_cache[4];
        float k_cache[4];
        float v_cache[4];
        
        // Write Q to cache
        int q_head = kv_head_offset;
        float q_pos = pos * kv_stride;

