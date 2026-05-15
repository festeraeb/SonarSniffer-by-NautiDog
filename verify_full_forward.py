#!/usr/bin/env python3
"""
Quick check: run full 28-layer forward pass for token 9707 at pos=0.
Only check the final hidden state RMS and logit[0] (the "!" token).
Skip attention (FFN-only) to match our Rust engine's current config.
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
        atype = read_u32(f)
        alen = read_u64(f)
        for _ in range(alen):
            if atype == 8:
                sl = read_u64(f); f.read(sl)
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

# Load all FFN weights for all 28 layers
print("Loading weights (this takes a few minutes)...")
ffn_norms = []
gates = []
ups = []
downs = []
for i in range(28):
    if i % 7 == 0: print(f"  Layer {i}/28...")
    fn, _ = load(f'blk.{i}.ffn_norm.weight')
    ffn_norms.append(fn)
    g, gs = load(f'blk.{i}.ffn_gate.weight')
    gates.append(g.reshape(gs[1], gs[0]))
    u, us = load(f'blk.{i}.ffn_up.weight')
    ups.append(u.reshape(us[1], us[0]))
    d, ds = load(f'blk.{i}.ffn_down.weight')
    downs.append(d.reshape(ds[1], ds[0]))

final_norm, _ = load('output_norm.weight')
lm_data, lm_shape = load('output.weight')
lm_head = lm_data.reshape(lm_shape[1], lm_shape[0])

print("Running 28-layer FFN-only forward pass...")
h = load_row(9707)
print(f"  Input rms: {np.sqrt(np.mean(h*h)):.4f}")

for i in range(28):
    normed = rmsnorm(h, ffn_norms[i])
    gate_out = gates[i] @ normed
    up_out = ups[i] @ normed
    silu_gate = gate_out * (1.0/(1.0+np.exp(-np.clip(gate_out,-88,88))))
    activated = silu_gate * up_out
    ffn_out = downs[i] @ activated
    h = h + ffn_out
    if i in (0, 13, 27):
        print(f"  Layer {i}: rms={np.sqrt(np.mean(h*h)):.4f} h[0:4]={h[:4]}")

# Final norm + lm_head
h_final = rmsnorm(h, final_norm)
logits = lm_head @ h_final
print(f"\n  Final norm h[0:4]: {h_final[:4]}")
print(f"  logit[0] (token '!'): {logits[0]:.4f}")
print(f"  logit max: {logits.max():.4f} at token {logits.argmax()}")
print(f"  Top-5: {sorted(enumerate(logits), key=lambda x:-x[1])[:5]}")
f.close()
