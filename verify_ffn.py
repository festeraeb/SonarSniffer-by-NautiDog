#!/usr/bin/env python3
"""
Minimal reference implementation: 1-layer FFN-only forward pass.
Loads the same GGUF model, runs embedding → FFN(layer0) → final_norm → lm_head → argmax.
Compare output token against our Rust engine to verify correctness.
"""
import struct
import numpy as np
import sys

GGUF_PATH = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"

# ── GGUF Parser (minimal, just what we need) ──────────────────────────────────

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
    if vtype == 0:  # UINT8
        return struct.unpack('B', f.read(1))[0]
    elif vtype == 1:  # INT8
        return struct.unpack('b', f.read(1))[0]
    elif vtype == 2:  # UINT16
        return struct.unpack('<H', f.read(2))[0]
    elif vtype == 3:  # INT16
        return struct.unpack('<h', f.read(2))[0]
    elif vtype == 4:  # UINT32
        return read_u32(f)
    elif vtype == 5:  # INT32
        return read_i32(f)
    elif vtype == 6:  # FLOAT32
        return read_f32(f)
    elif vtype == 7:  # BOOL
        return struct.unpack('?', f.read(1))[0]
    elif vtype == 8:  # STRING
        return read_string(f)
    elif vtype == 9:  # ARRAY
        arr_type = read_u32(f)
        arr_len = read_u64(f)
        arr = []
        for _ in range(arr_len):
            if arr_type == 8:
                arr.append(read_string(f))
            elif arr_type == 4:
                arr.append(read_u32(f))
            elif arr_type == 6:
                arr.append(read_f32(f))
            elif arr_type == 5:
                arr.append(read_i32(f))
            else:
                arr.append(f.read(1))
        return arr
    elif vtype == 10:  # UINT64
        return read_u64(f)
    elif vtype == 11:  # INT64
        return struct.unpack('<q', f.read(8))[0]
    elif vtype == 12:  # FLOAT64
        return struct.unpack('<d', f.read(8))[0]
    else:
        return None

def f16_to_f32(bits):
    """Convert IEEE 754 half-precision to float32."""
    return np.frombuffer(struct.pack('<H', bits), dtype=np.float16)[0].astype(np.float32)

def dequant_q6k_block(data, offset):
    """Dequantize one Q6_K block (256 elements) from raw bytes."""
    ql = data[offset:offset+128]
    qh = data[offset+128:offset+192]
    scales = data[offset+192:offset+208]
    d_bits = struct.unpack_from('<H', data, offset+208)[0]
    d = float(np.frombuffer(struct.pack('<H', d_bits), dtype=np.float16)[0])
    
    out = np.zeros(256, dtype=np.float32)
    for idx in range(256):
        ql_byte = ql[idx // 2]
        ql_val = (ql_byte & 0x0F) if (idx % 2 == 0) else ((ql_byte >> 4) & 0x0F)
        
        qh_byte = qh[idx // 4]
        qh_shift = (idx % 4) * 2
        qh_val = (qh_byte >> qh_shift) & 0x03
        
        q6 = (qh_val << 4) | ql_val
        quantized = q6 - 32
        
        sub_idx = idx // 16
        scale = np.array([scales[sub_idx]], dtype=np.uint8).view(np.int8)[0].astype(np.int32)
        
        out[idx] = d * float(scale) * float(quantized)
    
    return out

def dequant_q6k(data, n_elements):
    """Dequantize Q6_K tensor."""
    block_size = 256
    block_bytes = 210
    n_blocks = (n_elements + block_size - 1) // block_size
    out = np.zeros(n_elements, dtype=np.float32)
    
    for b in range(n_blocks):
        offset = b * block_bytes
        if offset + block_bytes > len(data):
            break
        block_out = dequant_q6k_block(data, offset)
        start = b * block_size
        end = min(start + block_size, n_elements)
        out[start:end] = block_out[:end-start]
    
    return out

def dequant_f16(data, n_elements):
    """Dequantize F16 tensor."""
    return np.frombuffer(data[:n_elements*2], dtype=np.float16).astype(np.float32)

def dequant_tensor(data, n_elements, quant_type):
    if quant_type == 0:  # F32
        return np.frombuffer(data[:n_elements*4], dtype=np.float32).copy()
    elif quant_type == 1 or quant_type == 30:  # F16
        return dequant_f16(data, n_elements)
    elif quant_type == 14:  # Q6_K
        return dequant_q6k(data, n_elements)
    else:
        print(f"  WARNING: unsupported quant type {quant_type}, using zeros")
        return np.zeros(n_elements, dtype=np.float32)

# ── Load Model ────────────────────────────────────────────────────────────────

print("Loading GGUF...")
f = open(GGUF_PATH, 'rb')

magic = read_u32(f)
assert magic == 0x46554747, f"Not GGUF: {magic:#x}"
version = read_u32(f)
n_tensors = read_u64(f)
n_metadata = read_u64(f)
print(f"  GGUF v{version}: {n_tensors} tensors, {n_metadata} metadata")

# Parse metadata
metadata = {}
for _ in range(n_metadata):
    key = read_string(f)
    val = read_value(f)
    metadata[key] = val

hidden_dim = metadata.get('qwen2.embedding_length', metadata.get('llama.embedding_length', 1536))
n_heads = metadata.get('qwen2.attention.head_count', metadata.get('llama.attention.head_count', 12))
vocab_size = metadata.get('qwen2.vocab_size', metadata.get('llama.vocab_size', 151936))
print(f"  hidden_dim={hidden_dim}, n_heads={n_heads}, vocab_size={vocab_size}")

# Parse tensor info
tensors = {}
for _ in range(n_tensors):
    name = read_string(f)
    n_dims = read_u32(f)
    shape = [read_u64(f) for _ in range(n_dims)]
    quant_type = read_u32(f)
    offset = read_u64(f)
    tensors[name] = {'shape': shape, 'quant_type': quant_type, 'offset': offset}

# Data offset (aligned to 32)
data_offset = (f.tell() + 31) & ~31
print(f"  data_offset={data_offset}")

def load_tensor(name):
    t = tensors[name]
    n_elements = 1
    for s in t['shape']:
        n_elements *= s
    f.seek(data_offset + t['offset'])
    # Compute byte size
    qt = t['quant_type']
    if qt == 0:
        nbytes = n_elements * 4
    elif qt in (1, 30):
        nbytes = n_elements * 2
    elif qt == 14:
        nbytes = ((n_elements + 255) // 256) * 210
    elif qt == 8:
        nbytes = n_elements + (n_elements // 32) * 2
    else:
        nbytes = n_elements * 2
    raw = f.read(nbytes)
    data = dequant_tensor(raw, n_elements, qt)
    return data, t['shape']

# ── Load required tensors ─────────────────────────────────────────────────────

print("\nLoading tensors...")
embed_data, embed_shape = load_tensor('token_embd.weight')
print(f"  token_embd: shape={embed_shape}, qt={tensors['token_embd.weight']['quant_type']}")
embed = embed_data.reshape(embed_shape[1], embed_shape[0])  # [vocab, hidden] after reshape
# Wait - GGUF shape is [ne0, ne1] = [hidden_dim, vocab_size] for embeddings
# So embed_shape = [1536, 151936]
# Reshape to [ne1, ne0] = [151936, 1536] for row-major access
print(f"  embed reshaped: {embed.shape}")

ffn_norm_data, _ = load_tensor('blk.0.ffn_norm.weight')
print(f"  ffn_norm: {ffn_norm_data.shape}, first 4: {ffn_norm_data[:4]}")

gate_data, gate_shape = load_tensor('blk.0.ffn_gate.weight')
print(f"  ffn_gate: gguf_shape={gate_shape}")
# GGUF shape [ne0, ne1] = [1536, 8960] — data is row-major with ne0 as fast dim
# So element at (i0, i1) is at flat index i1 * ne0 + i0
# For matvec: output[n] = sum_k(W[n * K + k] * input[k])
# n goes 0..8959 (ne1), k goes 0..1535 (ne0)
# W[n * 1536 + k] = element at (k, n) in GGUF notation = flat index n * ne0 + k ✓
gate_weight = gate_data.reshape(gate_shape[1], gate_shape[0])  # [8960, 1536]
print(f"  gate reshaped: {gate_weight.shape}")

up_data, up_shape = load_tensor('blk.0.ffn_up.weight')
up_weight = up_data.reshape(up_shape[1], up_shape[0])  # [8960, 1536]
print(f"  ffn_up: gguf_shape={up_shape}, reshaped: {up_weight.shape}")

down_data, down_shape = load_tensor('blk.0.ffn_down.weight')
down_weight = down_data.reshape(down_shape[1], down_shape[0])  # [1536, 8960]
print(f"  ffn_down: gguf_shape={down_shape}, reshaped: {down_weight.shape}")

final_norm_data, _ = load_tensor('output_norm.weight')
print(f"  output_norm: {final_norm_data.shape}")

# Check if output.weight exists or is tied
if 'output.weight' in tensors:
    lm_head_data, lm_shape = load_tensor('output.weight')
    lm_head = lm_head_data.reshape(lm_shape[1], lm_shape[0])  # [vocab, hidden]
    print(f"  output (lm_head): gguf_shape={lm_shape}, reshaped: {lm_head.shape}")
else:
    lm_head = embed  # Tied weights
    print(f"  lm_head: tied to token_embd")

# ── Forward Pass ──────────────────────────────────────────────────────────────

# Use token "Hello" — token ID for "Hello" in Qwen tokenizer
# Let's just use token 9707 ("Hello" in many tokenizers) or find it
# Actually let's use a simple token. Token 0 is <pad>, token 2 is <bos>
# Let's use the same prompt as our engine: "Hello"
# For simplicity, use token ID 9707 (common "Hello" token)
# Actually, let's just pick token 0 and see what happens
TEST_TOKEN = 9707  # "Hello" — adjust if needed

print(f"\n=== Forward pass with token {TEST_TOKEN} ===")

# 1. Embedding lookup
h = embed[TEST_TOKEN].copy()
print(f"  Embedding[0:8]: {h[:8]}")

# 2. RMSNorm with ffn_norm
def rmsnorm(x, weight, eps=1e-6):
    ms = np.mean(x * x)
    scale = 1.0 / np.sqrt(ms + eps)
    return x * scale * weight

h_normed = rmsnorm(h, ffn_norm_data)
print(f"  After FFN RMSNorm[0:8]: {h_normed[:8]}")

# 3. Gate and Up projections
gate_out = gate_weight @ h_normed  # [8960, 1536] @ [1536] = [8960]
up_out = up_weight @ h_normed      # [8960, 1536] @ [1536] = [8960]
print(f"  gate_out[0:4]: {gate_out[:4]}")
print(f"  up_out[0:4]: {up_out[:4]}")

# 4. SwiGLU: silu(gate) * up
def silu(x):
    return x * (1.0 / (1.0 + np.exp(-x)))

activated = silu(gate_out) * up_out
print(f"  SwiGLU[0:4]: {activated[:4]}")

# 5. Down projection
ffn_out = down_weight @ activated  # [1536, 8960] @ [8960] = [1536]
print(f"  FFN out[0:4]: {ffn_out[:4]}")

# 6. Residual add
h = h + ffn_out
print(f"  After residual[0:4]: {h[:4]}")

# 7. Final RMSNorm
h_final = rmsnorm(h, final_norm_data)
print(f"  After final norm[0:4]: {h_final[:4]}")

# 8. LM head projection
logits = lm_head @ h_final  # [vocab, hidden] @ [hidden] = [vocab]
print(f"  Logits[0:4]: {logits[:4]}")
print(f"  Logits max: {logits.max():.4f} at token {logits.argmax()}")

# 9. Argmax
next_token = int(logits.argmax())
print(f"\n  >>> PREDICTED TOKEN: {next_token}")
print(f"  (If our engine produces a different token, the computation is wrong)")

f.close()
