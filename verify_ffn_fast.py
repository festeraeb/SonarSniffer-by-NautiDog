#!/usr/bin/env python3
"""
Fast reference: 1-layer FFN-only forward pass.
Only dequants the specific embedding row we need + layer 0 FFN weights.
Uses vectorized numpy for Q6_K dequant.
"""
import struct
import numpy as np
import sys

GGUF_PATH = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"

def read_u32(f):
    return struct.unpack('<I', f.read(4))[0]

def read_u64(f):
    return struct.unpack('<Q', f.read(8))[0]

def read_i32(f):
    return struct.unpack('<i', f.read(4))[0]

def read_f32(f):
    return struct.unpack('<f', f.read(4))[0]

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
    """Vectorized Q6_K dequant using numpy."""
    block_size = 256
    block_bytes = 210
    n_blocks = (n_elements + block_size - 1) // block_size
    out = np.zeros(n_elements, dtype=np.float32)
    
    data = np.frombuffer(data_bytes[:n_blocks * block_bytes], dtype=np.uint8)
    
    for b in range(n_blocks):
        off = b * block_bytes
        if off + block_bytes > len(data):
            break
        
        ql = data[off:off+128]
        qh = data[off+128:off+192]
        scales_raw = data[off+192:off+208]
        d_bits = int(data[off+208]) | (int(data[off+209]) << 8)
        d = float(np.frombuffer(np.array([d_bits], dtype=np.uint16).tobytes(), dtype=np.float16)[0])
        
        # Vectorized extraction
        indices = np.arange(256)
        
        # ql: lower 4 bits, packed 2 per byte
        ql_bytes = ql[indices // 2]
        ql_vals = np.where(indices % 2 == 0, ql_bytes & 0x0F, (ql_bytes >> 4) & 0x0F)
        
        # qh: upper 2 bits, packed 4 per byte
        qh_bytes = qh[indices // 4]
        qh_shifts = (indices % 4) * 2
        qh_vals = (qh_bytes >> qh_shifts) & 0x03
        
        # Reconstruct 6-bit value
        q6 = (qh_vals.astype(np.int32) << 4) | ql_vals.astype(np.int32)
        quantized = q6 - 32
        
        # Sub-block scales (16 sub-blocks of 16 elements)
        sub_indices = indices // 16
        scales = scales_raw[sub_indices].view(np.int8).astype(np.int32)
        
        # Dequantize
        block_out = d * scales.astype(np.float32) * quantized.astype(np.float32)
        
        start = b * block_size
        end = min(start + block_size, n_elements)
        out[start:end] = block_out[:end-start]
    
    return out

def dequant_f16(data_bytes, n_elements):
    return np.frombuffer(data_bytes[:n_elements*2], dtype=np.float16).astype(np.float32)

def dequant_tensor(data_bytes, n_elements, quant_type):
    if quant_type == 0:
        return np.frombuffer(data_bytes[:n_elements*4], dtype=np.float32).copy()
    elif quant_type in (1, 30):
        return dequant_f16(data_bytes, n_elements)
    elif quant_type == 14:
        return dequant_q6k_vectorized(data_bytes, n_elements)
    else:
        print(f"  WARNING: unsupported quant type {quant_type}")
        return np.zeros(n_elements, dtype=np.float32)

def tensor_byte_size(n_elements, quant_type):
    if quant_type == 0: return n_elements * 4
    elif quant_type in (1, 30): return n_elements * 2
    elif quant_type == 14: return ((n_elements + 255) // 256) * 210
    elif quant_type == 8: return n_elements + ((n_elements + 31) // 32) * 2
    else: return n_elements * 2

# ── Load Model ────────────────────────────────────────────────────────────────

print("Loading GGUF...")
f = open(GGUF_PATH, 'rb')

magic = read_u32(f)
assert magic == 0x46554747, f"Not GGUF: {magic:#x}"
version = read_u32(f)
n_tensors = read_u64(f)
n_metadata = read_u64(f)
print(f"  GGUF v{version}: {n_tensors} tensors, {n_metadata} metadata")

metadata = {}
for _ in range(n_metadata):
    key = read_string(f)
    val = read_value(f)
    metadata[key] = val

hidden_dim = metadata.get('qwen2.embedding_length', metadata.get('llama.embedding_length', 1536))
print(f"  hidden_dim={hidden_dim}")

tensors = {}
for _ in range(n_tensors):
    name = read_string(f)
    n_dims = read_u32(f)
    shape = [read_u64(f) for _ in range(n_dims)]
    quant_type = read_u32(f)
    offset = read_u64(f)
    tensors[name] = {'shape': shape, 'quant_type': quant_type, 'offset': offset}

data_offset = (f.tell() + 31) & ~31
print(f"  data_offset={data_offset}")

def load_full_tensor(name):
    """Load and dequant a full tensor."""
    t = tensors[name]
    n_elements = 1
    for s in t['shape']:
        n_elements *= s
    nbytes = tensor_byte_size(n_elements, t['quant_type'])
    f.seek(data_offset + t['offset'])
    raw = f.read(nbytes)
    return dequant_tensor(raw, n_elements, t['quant_type']), t['shape']

def load_embedding_row(token_id):
    """Load just one row from the embedding table."""
    t = tensors['token_embd.weight']
    # GGUF shape [ne0, ne1] = [hidden_dim, vocab_size]
    # Each row (one token) is hidden_dim elements
    # Row token_id starts at element offset: token_id * ne0
    ne0 = t['shape'][0]  # hidden_dim = 1536
    row_elements = ne0
    qt = t['quant_type']
    
    if qt == 14:  # Q6_K
        # Q6_K blocks are 256 elements each
        # Row starts at element: token_id * ne0
        # Block index: (token_id * ne0) // 256
        # We need to dequant the blocks that contain our row
        row_start_elem = token_id * ne0
        row_end_elem = row_start_elem + ne0
        
        block_start = row_start_elem // 256
        block_end = (row_end_elem + 255) // 256
        
        # Read those blocks
        block_bytes = 210
        byte_offset = block_start * block_bytes
        n_blocks_needed = block_end - block_start
        
        f.seek(data_offset + t['offset'] + byte_offset)
        raw = f.read(n_blocks_needed * block_bytes)
        
        # Dequant those blocks
        n_elem_in_blocks = n_blocks_needed * 256
        full_data = dequant_q6k_vectorized(raw, n_elem_in_blocks)
        
        # Extract our row
        local_start = row_start_elem - block_start * 256
        return full_data[local_start:local_start + ne0]
    elif qt in (1, 30):  # F16
        byte_offset = token_id * ne0 * 2
        f.seek(data_offset + t['offset'] + byte_offset)
        raw = f.read(ne0 * 2)
        return np.frombuffer(raw, dtype=np.float16).astype(np.float32)
    elif qt == 0:  # F32
        byte_offset = token_id * ne0 * 4
        f.seek(data_offset + t['offset'] + byte_offset)
        raw = f.read(ne0 * 4)
        return np.frombuffer(raw, dtype=np.float32).copy()
    else:
        print(f"  Unsupported embedding quant: {qt}")
        return np.zeros(ne0, dtype=np.float32)

# ── Load tensors ──────────────────────────────────────────────────────────────

print("\nLoading tensors...")
print(f"  token_embd: shape={tensors['token_embd.weight']['shape']}, qt={tensors['token_embd.weight']['quant_type']}")
print(f"  blk.0.ffn_gate: shape={tensors['blk.0.ffn_gate.weight']['shape']}, qt={tensors['blk.0.ffn_gate.weight']['quant_type']}")
print(f"  blk.0.ffn_up: shape={tensors['blk.0.ffn_up.weight']['shape']}")
print(f"  blk.0.ffn_down: shape={tensors['blk.0.ffn_down.weight']['shape']}")

# Load 1D norm weights (small, fast)
ffn_norm, _ = load_full_tensor('blk.0.ffn_norm.weight')
final_norm, _ = load_full_tensor('output_norm.weight')
print(f"  ffn_norm[0:4]: {ffn_norm[:4]}")
print(f"  final_norm[0:4]: {final_norm[:4]}")

# Load FFN weight matrices
print("  Loading gate_proj (may take a moment)...")
gate_data, gate_shape = load_full_tensor('blk.0.ffn_gate.weight')
# GGUF shape [ne0=1536, ne1=8960], data in row-major with ne0 fast
# Reshape to [ne1, ne0] = [8960, 1536] for matrix multiply
gate_weight = gate_data.reshape(gate_shape[1], gate_shape[0])
print(f"  gate_weight: {gate_weight.shape}, [0,0:4]={gate_weight[0,:4]}")

print("  Loading up_proj...")
up_data, up_shape = load_full_tensor('blk.0.ffn_up.weight')
up_weight = up_data.reshape(up_shape[1], up_shape[0])
print(f"  up_weight: {up_weight.shape}")

print("  Loading down_proj...")
down_data, down_shape = load_full_tensor('blk.0.ffn_down.weight')
down_weight = down_data.reshape(down_shape[1], down_shape[0])
print(f"  down_weight: {down_weight.shape}")

# Load lm_head
print("  Loading lm_head...")
if 'output.weight' in tensors:
    lm_data, lm_shape = load_full_tensor('output.weight')
    lm_head = lm_data.reshape(lm_shape[1], lm_shape[0])
    print(f"  lm_head: {lm_head.shape}")
else:
    # Tied — need full embedding
    print("  lm_head tied to embeddings — loading full embed...")
    embed_data, embed_shape = load_full_tensor('token_embd.weight')
    lm_head = embed_data.reshape(embed_shape[1], embed_shape[0])
    print(f"  lm_head (tied): {lm_head.shape}")

# ── Forward Pass ──────────────────────────────────────────────────────────────

# Test with token "Hello" — let's find what token ID our engine uses
# The engine prompt starts with "Hello, I am CESARops..."
# For Qwen2.5 tokenizer, "Hello" is likely token 9707 or similar
# Let's test with a few tokens
TEST_TOKENS = [9707, 0, 1, 2, 151643]

for TEST_TOKEN in TEST_TOKENS[:2]:  # Just test first 2
    print(f"\n{'='*60}")
    print(f"=== Forward pass: token {TEST_TOKEN} ===")
    
    # 1. Embedding
    h = load_embedding_row(TEST_TOKEN)
    print(f"  embed[0:8]: {h[:8]}")
    
    # 2. RMSNorm (ffn_norm)
    def rmsnorm(x, weight, eps=1e-6):
        ms = np.mean(x * x)
        scale = 1.0 / np.sqrt(ms + eps)
        return x * scale * weight
    
    h_normed = rmsnorm(h, ffn_norm)
    print(f"  normed[0:4]: {h_normed[:4]}")
    
    # 3. Gate + Up projections
    gate_out = gate_weight @ h_normed
    up_out = up_weight @ h_normed
    print(f"  gate[0:4]: {gate_out[:4]}")
    print(f"  up[0:4]: {up_out[:4]}")
    
    # 4. SwiGLU
    silu_gate = gate_out * (1.0 / (1.0 + np.exp(-np.clip(gate_out, -88, 88))))
    activated = silu_gate * up_out
    print(f"  swiglu[0:4]: {activated[:4]}")
    
    # 5. Down projection
    ffn_out = down_weight @ activated
    print(f"  ffn_out[0:4]: {ffn_out[:4]}")
    
    # 6. Residual
    h_res = h + ffn_out
    print(f"  residual[0:4]: {h_res[:4]}")
    
    # 7. Final norm
    h_final = rmsnorm(h_res, final_norm)
    print(f"  final[0:4]: {h_final[:4]}")
    
    # 8. LM head
    logits = lm_head @ h_final
    top_token = int(logits.argmax())
    print(f"  logits max={logits.max():.4f} at token {top_token}")
    print(f"  >>> REFERENCE OUTPUT TOKEN: {top_token}")

f.close()
print("\nDone. Compare these token IDs against the Rust engine output.")
