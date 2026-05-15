#!/usr/bin/env python3
"""
Reference attention computation for layer 0, token 0 (pos=0).
At pos=0 with 1 token, attention is trivial: softmax([score]) = [1.0], context = V[0].
The real test is the QKV projection + RoPE + O_proj path.

For pos=0:
  Q = W_q @ normed + bias_q    [1536]
  K = W_k @ normed + bias_k    [256]  (n_kv_heads * head_dim = 2*128)
  V = W_v @ normed + bias_v    [256]
  Q_rope = apply_rope(Q, pos=0)
  K_rope = apply_rope(K, pos=0)
  
  For each head h (0..11):
    kv_head = h // 6
    q_h = Q_rope[h*128 : (h+1)*128]
    k_h = K_rope[kv_head*128 : (kv_head+1)*128]
    score = dot(q_h, k_h) / sqrt(128)
    prob = softmax([score]) = [1.0]  (only 1 position)
    v_h = V[kv_head*128 : (kv_head+1)*128]
    attn_out_h = v_h  (weighted by prob=1.0)
  
  attn_out = concat all heads [1536]
  projected = W_o @ attn_out   [1536]
  hidden = embedding + projected  (residual)

Then FFN runs on the result.
"""
import struct
import numpy as np

GGUF_PATH = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"

def read_u32(f): return struct.unpack('<I', f.read(4))[0]
def read_u64(f): return struct.unpack('<Q', f.read(8))[0]
def read_i32(f): return struct.unpack('<i', f.read(4))[0]
def read_f32(f): return struct.unpack('<f', f.read(4))[0]
def read_string(f):
    length = read_u64(f)
    return f.read(length).decode('utf-8')

def read_value(f):
    vtype = read_u32(f)
    if vtype == 0: return struct.unpack('B', f.read(1))[0]
    elif vtype == 1: return struct.unpack('b', f.read(1))[0]
    elif vtype == 2: return struct.unpack('<H', f.read(2))[0]
    elif vtype == 3: return struct.unpack('<h', f.read(2))[0]
    elif vtype == 4: return read_u32(f)
    elif vtype == 5: return read_i32(f)
    elif vtype == 6: return read_f32(f)
    elif vtype == 7: return struct.unpack('?', f.read(1))[0]
    elif vtype == 8: return read_string(f)
    elif vtype == 9:
        arr_type = read_u32(f)
        arr_len = read_u64(f)
        arr = []
        for _ in range(arr_len):
            if arr_type == 8: arr.append(read_string(f))
            elif arr_type == 4: arr.append(read_u32(f))
            elif arr_type == 6: arr.append(read_f32(f))
            elif arr_type == 5: arr.append(read_i32(f))
            else: arr.append(f.read(1))
        return arr
    elif vtype == 10: return read_u64(f)
    elif vtype == 11: return struct.unpack('<q', f.read(8))[0]
    elif vtype == 12: return struct.unpack('<d', f.read(8))[0]
    else: return None

def dequant_q6k_vectorized(data_bytes, n_elements):
    block_size = 256
    block_bytes = 210
    n_blocks = (n_elements + block_size - 1) // block_size
    out = np.zeros(n_elements, dtype=np.float32)
    data = np.frombuffer(data_bytes[:n_blocks * block_bytes], dtype=np.uint8)
    for b in range(n_blocks):
        off = b * block_bytes
        if off + block_bytes > len(data): break
        ql = data[off:off+128]
        qh = data[off+128:off+192]
        scales_raw = data[off+192:off+208]
        d_bits = int(data[off+208]) | (int(data[off+209]) << 8)
        d = float(np.frombuffer(np.array([d_bits], dtype=np.uint16).tobytes(), dtype=np.float16)[0])
        indices = np.arange(256)
        ql_bytes = ql[indices // 2]
        ql_vals = np.where(indices % 2 == 0, ql_bytes & 0x0F, (ql_bytes >> 4) & 0x0F)
        qh_bytes = qh[indices // 4]
        qh_shifts = (indices % 4) * 2
        qh_vals = (qh_bytes >> qh_shifts) & 0x03
        q6 = (qh_vals.astype(np.int32) << 4) | ql_vals.astype(np.int32)
        quantized = q6 - 32
        sub_indices = indices // 16
        scales = scales_raw[sub_indices].view(np.int8).astype(np.int32)
        block_out = d * scales.astype(np.float32) * quantized.astype(np.float32)
        start = b * block_size
        end = min(start + block_size, n_elements)
        out[start:end] = block_out[:end-start]
    return out

def dequant_f16(data_bytes, n_elements):
    return np.frombuffer(data_bytes[:n_elements*2], dtype=np.float16).astype(np.float32)

def dequant_tensor(data_bytes, n_elements, quant_type):
    if quant_type == 0: return np.frombuffer(data_bytes[:n_elements*4], dtype=np.float32).copy()
    elif quant_type in (1, 30): return dequant_f16(data_bytes, n_elements)
    elif quant_type == 14: return dequant_q6k_vectorized(data_bytes, n_elements)
    else: return np.zeros(n_elements, dtype=np.float32)

def tensor_byte_size(n_elements, quant_type):
    if quant_type == 0: return n_elements * 4
    elif quant_type in (1, 30): return n_elements * 2
    elif quant_type == 14: return ((n_elements + 255) // 256) * 210
    elif quant_type == 8: return n_elements + ((n_elements + 31) // 32) * 2
    else: return n_elements * 2

def apply_rope(x, pos, head_dim, n_heads, theta_base=1000000.0):
    """Apply RoPE rotation. x is [n_heads * head_dim]. Pairs are (d, d+half)."""
    out = x.copy()
    half = head_dim // 2
    for h in range(n_heads):
        base = h * head_dim
        for d in range(half):
            freq_exp = -2.0 * d / head_dim
            theta = pos * (theta_base ** freq_exp)
            cos_t = np.cos(theta)
            sin_t = np.sin(theta)
            x0 = x[base + d]
            x1 = x[base + d + half]
            out[base + d] = x0 * cos_t - x1 * sin_t
            out[base + d + half] = x0 * sin_t + x1 * cos_t
    return out

# ── Load Model ────────────────────────────────────────────────────────────────
print("Loading GGUF...")
f = open(GGUF_PATH, 'rb')
magic = read_u32(f)
assert magic == 0x46554747
version = read_u32(f)
n_tensors = read_u64(f)
n_metadata = read_u64(f)

metadata = {}
for _ in range(n_metadata):
    key = read_string(f)
    val = read_value(f)
    metadata[key] = val

tensors = {}
for _ in range(n_tensors):
    name = read_string(f)
    n_dims = read_u32(f)
    shape = [read_u64(f) for _ in range(n_dims)]
    quant_type = read_u32(f)
    offset = read_u64(f)
    tensors[name] = {'shape': shape, 'quant_type': quant_type, 'offset': offset}

data_offset = (f.tell() + 31) & ~31

def load_full_tensor(name):
    t = tensors[name]
    n_elements = 1
    for s in t['shape']: n_elements *= s
    nbytes = tensor_byte_size(n_elements, t['quant_type'])
    f.seek(data_offset + t['offset'])
    raw = f.read(nbytes)
    return dequant_tensor(raw, n_elements, t['quant_type']), t['shape']

def load_embedding_row(token_id):
    t = tensors['token_embd.weight']
    ne0 = t['shape'][0]
    qt = t['quant_type']
    if qt == 14:
        row_start_elem = token_id * ne0
        block_start = row_start_elem // 256
        block_end = (row_start_elem + ne0 + 255) // 256
        block_bytes_size = 210
        byte_offset = block_start * block_bytes_size
        n_blocks_needed = block_end - block_start
        f.seek(data_offset + t['offset'] + byte_offset)
        raw = f.read(n_blocks_needed * block_bytes_size)
        full_data = dequant_q6k_vectorized(raw, n_blocks_needed * 256)
        local_start = row_start_elem - block_start * 256
        return full_data[local_start:local_start + ne0]
    elif qt in (1, 30):
        f.seek(data_offset + t['offset'] + token_id * ne0 * 2)
        return np.frombuffer(f.read(ne0 * 2), dtype=np.float16).astype(np.float32)
    else:
        f.seek(data_offset + t['offset'] + token_id * ne0 * 4)
        return np.frombuffer(f.read(ne0 * 4), dtype=np.float32).copy()

# ── Load tensors ──────────────────────────────────────────────────────────────
print("Loading attention weights for layer 0...")
attn_norm, _ = load_full_tensor('blk.0.attn_norm.weight')
print(f"  attn_norm[0:4]: {attn_norm[:4]}")

print("  Loading Q proj...")
q_data, q_shape = load_full_tensor('blk.0.attn_q.weight')
q_weight = q_data.reshape(q_shape[1], q_shape[0])  # [1536, 1536]
print(f"  q_weight: {q_weight.shape}")

print("  Loading K proj...")
k_data, k_shape = load_full_tensor('blk.0.attn_k.weight')
k_weight = k_data.reshape(k_shape[1], k_shape[0])  # [256, 1536]
print(f"  k_weight: {k_weight.shape}")

print("  Loading V proj...")
v_data, v_shape = load_full_tensor('blk.0.attn_v.weight')
v_weight = v_data.reshape(v_shape[1], v_shape[0])  # [256, 1536]
print(f"  v_weight: {v_weight.shape}")

print("  Loading O proj...")
o_data, o_shape = load_full_tensor('blk.0.attn_output.weight')
o_weight = o_data.reshape(o_shape[1], o_shape[0])  # [1536, 1536]
print(f"  o_weight: {o_weight.shape}")

# Load biases
q_bias_data, _ = load_full_tensor('blk.0.attn_q.bias')
k_bias_data, _ = load_full_tensor('blk.0.attn_k.bias')
v_bias_data, _ = load_full_tensor('blk.0.attn_v.bias')
print(f"  q_bias[0:4]: {q_bias_data[:4]}")
print(f"  k_bias[0:4]: {k_bias_data[:4]}")
print(f"  v_bias[0:4]: {v_bias_data[:4]}")

# ── Forward pass: attention at pos=0 ─────────────────────────────────────────
TEST_TOKEN = 9707  # "Hello"
print(f"\n=== Attention reference for token {TEST_TOKEN}, pos=0 ===")

h = load_embedding_row(TEST_TOKEN)
print(f"  embed[0:4]: {h[:4]}")

# RMSNorm with attn_norm
def rmsnorm(x, weight, eps=1e-6):
    ms = np.mean(x * x)
    scale = 1.0 / np.sqrt(ms + eps)
    return x * scale * weight

normed = rmsnorm(h, attn_norm)
print(f"  attn_normed[0:4]: {normed[:4]}")

# QKV projections
Q = q_weight @ normed + q_bias_data  # [1536]
K = k_weight @ normed + k_bias_data  # [256]
V = v_weight @ normed + v_bias_data  # [256]
print(f"  Q[0:4]: {Q[:4]}")
print(f"  K[0:4]: {K[:4]}")
print(f"  V[0:4]: {V[:4]}")

# RoPE at pos=0
# At pos=0, theta = 0 * base^(...) = 0 for all dims
# cos(0) = 1, sin(0) = 0
# So RoPE at pos=0 is identity! Q_rope = Q, K_rope = K
Q_rope = apply_rope(Q, 0, 128, 12)
K_rope = apply_rope(K, 0, 128, 2)
print(f"  Q_rope[0:4]: {Q_rope[:4]}  (should equal Q since pos=0)")
print(f"  K_rope[0:4]: {K_rope[:4]}  (should equal K since pos=0)")

# At pos=0 with only 1 token, attention is trivial:
# For each head, the only key is K[kv_head], score = Q.K/sqrt(d), softmax = [1.0]
# So attn_out[h] = V[kv_head]
attn_out = np.zeros(1536, dtype=np.float32)
for h_idx in range(12):
    kv_head = h_idx // 6
    v_h = V[kv_head*128 : (kv_head+1)*128]
    attn_out[h_idx*128 : (h_idx+1)*128] = v_h

print(f"  attn_out[0:4]: {attn_out[:4]}  (= V[head0][0:4] for heads 0-5)")
print(f"  attn_out[768:772]: {attn_out[768:772]}  (= V[head1][0:4] for heads 6-11)")

# O projection
projected = o_weight @ attn_out
print(f"  O_proj[0:4]: {projected[:4]}")

# Attention residual
h_after_attn = h + projected
print(f"  After attn residual[0:4]: {h_after_attn[:4]}")

# Now FFN on h_after_attn
ffn_norm, _ = load_full_tensor('blk.0.ffn_norm.weight')
ffn_normed = rmsnorm(h_after_attn, ffn_norm)
print(f"  FFN normed[0:4]: {ffn_normed[:4]}")

print("\n=== KEY VALUES FOR RUST ENGINE COMPARISON ===")
print(f"  attn_normed[0:4] = {list(normed[:4])}")
print(f"  Q[0:4] = {list(Q[:4])}")
print(f"  K[0:4] = {list(K[:4])}")
print(f"  V[0:4] = {list(V[:4])}")
print(f"  attn_out[0:4] = {list(attn_out[:4])}")
print(f"  O_proj[0:4] = {list(projected[:4])}")
print(f"  h_after_attn[0:4] = {list(h_after_attn[:4])}")

f.close()
print("\nDone.")
