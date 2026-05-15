#!/usr/bin/env python3
"""
Full 28-layer forward pass for the SECOND token (pos=1).
First token is 9707 ("Hello"), second token is 135144 (what the model generated).
At pos=1, attention has kv_len=2 and must attend to both positions.
This includes RoPE at pos=0 and pos=1.
"""
import struct, numpy as np, sys

GGUF_PATH = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"

def read_u32(f): return struct.unpack('<I', f.read(4))[0]
def read_u64(f): return struct.unpack('<Q', f.read(8))[0]
def read_string(f):
    length = read_u64(f)
    return f.read(length).decode('utf-8')
def read_value(f):
    vtype = read_u32(f)
    if vtype == 4: return read_u32(f)
    elif vtype == 5: return struct.unpack('<i', f.read(4))[0]
    elif vtype == 6: return struct.unpack('<f', f.read(4))[0]
    elif vtype == 8: return read_string(f)
    elif vtype == 9:
        atype = read_u32(f); alen = read_u64(f)
        for _ in range(alen):
            if atype == 8: sl = read_u64(f); f.read(sl)
            elif atype in (4,5,6): f.read(4)
            elif atype in (0,1): f.read(1)
            elif atype in (2,3): f.read(2)
            elif atype in (10,11,12): f.read(8)
            else: f.read(4)
        return None
    elif vtype == 10: return read_u64(f)
    elif vtype in (0,1): return struct.unpack('B', f.read(1))[0]
    elif vtype in (2,3): return struct.unpack('<H', f.read(2))[0]
    elif vtype == 7: return struct.unpack('?', f.read(1))[0]
    elif vtype in (11,12): f.read(8); return None
    else: return None

def dequant_q6k(data_bytes, n_elements):
    block_size, block_bytes = 256, 210
    n_blocks = (n_elements + block_size - 1) // block_size
    out = np.zeros(n_elements, dtype=np.float32)
    data = np.frombuffer(data_bytes[:n_blocks * block_bytes], dtype=np.uint8)
    for b in range(n_blocks):
        off = b * block_bytes
        if off + block_bytes > len(data): break
        ql = data[off:off+128]; qh = data[off+128:off+192]
        scales_raw = data[off+192:off+208]
        d = float(np.frombuffer(np.array([int(data[off+208])|(int(data[off+209])<<8)], dtype=np.uint16).tobytes(), dtype=np.float16)[0])
        indices = np.arange(256)
        ql_vals = np.where(indices%2==0, ql[indices//2]&0x0F, (ql[indices//2]>>4)&0x0F)
        qh_vals = (qh[indices//4]>>((indices%4)*2))&0x03
        q6 = (qh_vals.astype(np.int32)<<4)|ql_vals.astype(np.int32)
        scales = scales_raw[indices//16].view(np.int8).astype(np.int32)
        block_out = d * scales.astype(np.float32) * (q6-32).astype(np.float32)
        start = b*block_size; end = min(start+block_size, n_elements)
        out[start:end] = block_out[:end-start]
    return out

def tensor_bytes(n_elements, qt):
    if qt == 0: return n_elements*4
    elif qt in (1,30): return n_elements*2
    elif qt == 14: return ((n_elements+255)//256)*210
    else: return n_elements*2

def dequant(raw, n_elements, qt):
    if qt == 0: return np.frombuffer(raw[:n_elements*4], dtype=np.float32).copy()
    elif qt in (1,30): return np.frombuffer(raw[:n_elements*2], dtype=np.float16).astype(np.float32)
    elif qt == 14: return dequant_q6k(raw, n_elements)
    else: return np.zeros(n_elements, dtype=np.float32)

def apply_rope(x, pos, head_dim, n_heads, theta_base=1000000.0):
    """Apply RoPE. Half-split style: pairs are (d, d+half_dim)."""
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

def rmsnorm(x, w, eps=1e-6):
    return x * (1.0/np.sqrt(np.mean(x*x)+eps)) * w

print("Loading GGUF...")
f = open(GGUF_PATH, 'rb')
f.read(4); f.read(4)
nt = read_u64(f); nm = read_u64(f)
metadata = {}
for _ in range(nm):
    key = read_string(f); val = read_value(f)
    if val is not None: metadata[key] = val
tensors = {}
for _ in range(nt):
    name = read_string(f); nd = read_u32(f)
    shape = [read_u64(f) for _ in range(nd)]
    qt = read_u32(f); offset = read_u64(f)
    tensors[name] = {'shape':shape,'qt':qt,'offset':offset}
data_offset = (f.tell()+31)&~31

def load(name):
    t = tensors[name]; ne = 1
    for s in t['shape']: ne *= s
    nb = tensor_bytes(ne, t['qt'])
    f.seek(data_offset + t['offset'])
    return dequant(f.read(nb), ne, t['qt']), t['shape']

def load_row(token_id):
    t = tensors['token_embd.weight']; ne0 = t['shape'][0]; qt = t['qt']
    row_start = token_id * ne0
    bs, bb = 256, 210
    b0 = row_start//bs; b1 = (row_start+ne0+bs-1)//bs
    f.seek(data_offset + t['offset'] + b0*bb)
    raw = f.read((b1-b0)*bb)
    full = dequant_q6k(raw, (b1-b0)*bs)
    return full[row_start-b0*bs:row_start-b0*bs+ne0]

# Load ALL weights
print("Loading weights...")
attn_norms = []; ffn_norms = []
q_projs = []; k_projs = []; v_projs = []; o_projs = []
q_biases = []; k_biases = []; v_biases = []
gates = []; ups = []; downs = []

for i in range(28):
    if i % 7 == 0: print(f"  Layer {i}/28...")
    an, _ = load(f'blk.{i}.attn_norm.weight'); attn_norms.append(an)
    fn, _ = load(f'blk.{i}.ffn_norm.weight'); ffn_norms.append(fn)
    q, qs = load(f'blk.{i}.attn_q.weight'); q_projs.append(q.reshape(qs[1], qs[0]))
    k, ks = load(f'blk.{i}.attn_k.weight'); k_projs.append(k.reshape(ks[1], ks[0]))
    v, vs = load(f'blk.{i}.attn_v.weight'); v_projs.append(v.reshape(vs[1], vs[0]))
    o, os = load(f'blk.{i}.attn_output.weight'); o_projs.append(o.reshape(os[1], os[0]))
    qb, _ = load(f'blk.{i}.attn_q.bias'); q_biases.append(qb)
    kb, _ = load(f'blk.{i}.attn_k.bias'); k_biases.append(kb)
    vb, _ = load(f'blk.{i}.attn_v.bias'); v_biases.append(vb)
    g, gs = load(f'blk.{i}.ffn_gate.weight'); gates.append(g.reshape(gs[1], gs[0]))
    u, us = load(f'blk.{i}.ffn_up.weight'); ups.append(u.reshape(us[1], us[0]))
    d, ds = load(f'blk.{i}.ffn_down.weight'); downs.append(d.reshape(ds[1], ds[0]))

final_norm, _ = load('output_norm.weight')
lm_data, lm_shape = load('output.weight')
lm_head = lm_data.reshape(lm_shape[1], lm_shape[0])

# ── PREFILL: Process token 9707 at pos=0 ─────────────────────────────────────
print("\n=== PREFILL: token 9707 at pos=0 ===")
h0 = load_row(9707)

# KV cache: store K and V for each layer at pos=0
kv_cache_k = []  # [layer][pos] = K_rope vector [256]
kv_cache_v = []  # [layer][pos] = V vector [256]

for i in range(28):
    normed = rmsnorm(h0, attn_norms[i])
    Q = q_projs[i] @ normed + q_biases[i]
    K = k_projs[i] @ normed + k_biases[i]
    V = v_projs[i] @ normed + v_biases[i]
    # RoPE at pos=0 is identity
    Q_rope = apply_rope(Q, 0, 128, 12)
    K_rope = apply_rope(K, 0, 128, 2)
    # Store in KV cache
    kv_cache_k.append([K_rope.copy()])
    kv_cache_v.append([V.copy()])
    # Attention (trivial: 1 position)
    attn_out = np.zeros(1536, dtype=np.float32)
    for head in range(12):
        kv_head = head // 6
        attn_out[head*128:(head+1)*128] = V[kv_head*128:(kv_head+1)*128]
    projected = o_projs[i] @ attn_out
    h0 = h0 + projected
    # FFN
    normed2 = rmsnorm(h0, ffn_norms[i])
    gate_out = gates[i] @ normed2
    up_out = ups[i] @ normed2
    silu_gate = gate_out * (1.0/(1.0+np.exp(-np.clip(gate_out,-88,88))))
    ffn_out = downs[i] @ (silu_gate * up_out)
    h0 = h0 + ffn_out

print(f"  After 28 layers: rms={np.sqrt(np.mean(h0*h0)):.4f}")

# ── DECODE: Process token 135144 at pos=1 ─────────────────────────────────────
print("\n=== DECODE: token 135144 at pos=1 ===")
h1 = load_row(135144)
print(f"  Input embed rms: {np.sqrt(np.mean(h1*h1)):.4f}")

for i in range(28):
    normed = rmsnorm(h1, attn_norms[i])
    Q = q_projs[i] @ normed + q_biases[i]
    K = k_projs[i] @ normed + k_biases[i]
    V = v_projs[i] @ normed + v_biases[i]
    # RoPE at pos=1
    Q_rope = apply_rope(Q, 1, 128, 12)
    K_rope = apply_rope(K, 1, 128, 2)
    # Store in KV cache at pos=1
    kv_cache_k[i].append(K_rope.copy())
    kv_cache_v[i].append(V.copy())

    # Attention with kv_len=2
    attn_out = np.zeros(1536, dtype=np.float32)
    scale = 1.0 / np.sqrt(128.0)
    for head in range(12):
        kv_head = head // 6
        q_h = Q_rope[head*128:(head+1)*128]
        # Scores against both positions
        k0 = kv_cache_k[i][0][kv_head*128:(kv_head+1)*128]
        k1 = kv_cache_k[i][1][kv_head*128:(kv_head+1)*128]
        score0 = np.dot(q_h, k0) * scale
        score1 = np.dot(q_h, k1) * scale
        # Softmax
        max_s = max(score0, score1)
        e0 = np.exp(score0 - max_s)
        e1 = np.exp(score1 - max_s)
        sum_e = e0 + e1
        p0 = e0 / sum_e
        p1 = e1 / sum_e
        # Weighted sum of V
        v0 = kv_cache_v[i][0][kv_head*128:(kv_head+1)*128]
        v1 = kv_cache_v[i][1][kv_head*128:(kv_head+1)*128]
        context = p0 * v0 + p1 * v1
        attn_out[head*128:(head+1)*128] = context

        if i == 0 and head == 0:
            print(f"  [L0 H0] scores=[{score0:.4f}, {score1:.4f}] probs=[{p0:.4f}, {p1:.4f}]")
            print(f"  [L0 H0] context[0:4]={context[:4]}")

    projected = o_projs[i] @ attn_out
    h1 = h1 + projected
    # FFN
    normed2 = rmsnorm(h1, ffn_norms[i])
    gate_out = gates[i] @ normed2
    up_out = ups[i] @ normed2
    silu_gate = gate_out * (1.0/(1.0+np.exp(-np.clip(gate_out,-88,88))))
    ffn_out = downs[i] @ (silu_gate * up_out)
    h1 = h1 + ffn_out

    if i in (0, 13, 27):
        print(f"  Layer {i}: rms={np.sqrt(np.mean(h1*h1)):.4f} h[0:4]={h1[:4]}")

# Final norm + lm_head
h_final = rmsnorm(h1, final_norm)
logits = lm_head @ h_final
print(f"\n  Final norm h[0:4]: {h_final[:4]}")
print(f"  logit[0] (token '!'): {logits[0]:.4f}")
print(f"  logit max: {logits.max():.4f} at token {logits.argmax()}")
top5 = sorted(enumerate(logits), key=lambda x:-x[1])[:5]
print(f"  Top-5: {[(t,f'{v:.4f}') for t,v in top5]}")

print("\n=== COMPARE THESE VALUES AGAINST RUST ENGINE ===")
print("If Rust engine layer 0 head 0 scores match, attention is correct.")
print("If they don't match, the bug is in RoPE or KV cache stride at pos>0.")
f.close()
