#!/usr/bin/env python3
"""
Full 28-layer forward pass WITH ATTENTION for token 9707 at pos=0.
At pos=0 with 1 token, attention is trivial: softmax([score])=[1.0], output=V.
So attn_out = V for each head, then O_proj, then residual add.
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

def rmsnorm(x, w, eps=1e-6):
    return x * (1.0/np.sqrt(np.mean(x*x)+eps)) * w

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

# Forward pass with attention (pos=0, trivial: 1 position)
print("\nRunning 28-layer forward pass WITH ATTENTION (pos=0)...")
h = load_row(9707)
print(f"  Input rms: {np.sqrt(np.mean(h*h)):.4f}")

for i in range(28):
    # Attention block
    normed = rmsnorm(h, attn_norms[i])
    Q = q_projs[i] @ normed + q_biases[i]  # [1536]
    K = k_projs[i] @ normed + k_biases[i]  # [256]
    V = v_projs[i] @ normed + v_biases[i]  # [256]
    # RoPE at pos=0 is identity (theta=0, cos=1, sin=0)
    # Attention with 1 position: output = V for each head
    attn_out = np.zeros(1536, dtype=np.float32)
    for head in range(12):
        kv_head = head // 6
        attn_out[head*128:(head+1)*128] = V[kv_head*128:(kv_head+1)*128]
    projected = o_projs[i] @ attn_out
    h = h + projected  # attention residual

    # FFN block
    normed2 = rmsnorm(h, ffn_norms[i])
    gate_out = gates[i] @ normed2
    up_out = ups[i] @ normed2
    silu_gate = gate_out * (1.0/(1.0+np.exp(-np.clip(gate_out,-88,88))))
    ffn_out = downs[i] @ (silu_gate * up_out)
    h = h + ffn_out  # FFN residual

    if i in (0, 13, 27):
        print(f"  Layer {i}: rms={np.sqrt(np.mean(h*h)):.4f} h[0:4]={h[:4]}")

# Final norm + lm_head
h_final = rmsnorm(h, final_norm)
logits = lm_head @ h_final
print(f"\n  Final norm h[0:4]: {h_final[:4]}")
print(f"  logit[0] (token '!'): {logits[0]:.4f}")
print(f"  logit max: {logits.max():.4f} at token {logits.argmax()}")
top5 = sorted(enumerate(logits), key=lambda x:-x[1])[:5]
print(f"  Top-5: {[(t,f'{v:.4f}') for t,v in top5]}")
f.close()
