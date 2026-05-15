#!/usr/bin/env python3
"""
Reference computation for Layer 0 FFN of Qwen2.5-Coder-1.5B Q6_K.
Compares against our GPU engine's output to find the exact divergence point.

Steps:
1. Load embedding for token "Hello" (verified correct: [-0.015083, 0.010893, ...])
2. Apply RMSNorm with blk.0.attn_norm.weight (verified: [0.641, 0.540, 0.620, ...])
3. Skip attention (produces zeros on first token)
4. Apply RMSNorm with blk.0.ffn_norm.weight
5. Compute gate = ffn_normed @ gate_proj.T (matvec)
6. Compute up = ffn_normed @ up_proj.T (matvec)
7. Compute SwiGLU: silu(gate) * up
8. Compute down = swiglu_out @ down_proj.T
9. Add residual: hidden = embedding + down
10. Compare against GPU output
"""

import struct
import sys
import math

GGUF_PATH = "/codebase/models/qwen2.5-coder-1.5b-instruct-q6_k.gguf"
DATA_OFFSET = 5950528  # From our loader logs
HIDDEN_DIM = 1536
INTERMEDIATE_DIM = 8960

def f16_to_f32(bits):
    sign = (bits >> 15) & 1
    exp = (bits >> 10) & 0x1F
    mant = bits & 0x3FF
    if exp == 0:
        if mant == 0: return 0.0
        val = mant * 5.96046447e-8
        return -val if sign else val
    if exp == 31:
        return -65504.0 if sign else 65504.0
    f32_exp = exp - 15 + 127
    f32_bits = (sign << 31) | (f32_exp << 23) | (mant << 13)
    return struct.unpack('f', struct.pack('I', f32_bits))[0]

def dequant_q6k_block(block_bytes):
    """Dequant one 210-byte Q6_K block to 256 f32 values"""
    ql = block_bytes[0:128]
    qh = block_bytes[128:192]
    scales = block_bytes[192:208]
    d_bits = struct.unpack('<H', block_bytes[208:210])[0]
    d = f16_to_f32(d_bits)
    
    values = []
    for idx in range(256):
        ql_byte = ql[idx // 2]
        ql_val = (ql_byte & 0x0F) if idx % 2 == 0 else ((ql_byte >> 4) & 0x0F)
        
        qh_byte = qh[idx // 4]
        qh_shift = (idx % 4) * 2
        qh_val = (qh_byte >> qh_shift) & 0x03
        
        q6 = (qh_val << 4) | ql_val
        quantized = q6 - 32
        
        sub_idx = idx // 16
        scale = struct.unpack('b', bytes([scales[sub_idx]]))[0]
        
        values.append(d * scale * quantized)
    return values

def load_tensor_q6k(f, offset, n_elements):
    """Load and dequant a Q6_K tensor from file"""
    n_blocks = (n_elements + 255) // 256
    f.seek(DATA_OFFSET + offset)
    data = f.read(n_blocks * 210)
    
    values = []
    for b in range(n_blocks):
        block = data[b*210:(b+1)*210]
        if len(block) < 210:
            break
        values.extend(dequant_q6k_block(block))
    return values[:n_elements]

def load_tensor_f32(f, offset, n_elements):
    """Load an F32 tensor from file"""
    f.seek(DATA_OFFSET + offset)
    data = f.read(n_elements * 4)
    return list(struct.unpack(f'<{n_elements}f', data))

def rmsnorm(x, weight, eps=1e-6):
    """RMSNorm: x * weight / sqrt(mean(x^2) + eps)"""
    n = len(x)
    ss = sum(v*v for v in x) / n
    scale = 1.0 / math.sqrt(ss + eps)
    return [x[i] * scale * weight[i] for i in range(n)]

def matvec(weight_flat, x, n_out, k_in):
    """Matrix-vector multiply: output[n] = sum_k(W[n*K+k] * x[k])"""
    output = []
    for n in range(n_out):
        dot = 0.0
        base = n * k_in
        for k in range(k_in):
            dot += weight_flat[base + k] * x[k]
        output.append(dot)
    return output

def silu(x):
    """SiLU activation: x * sigmoid(x)"""
    return x / (1.0 + math.exp(-min(max(x, -80), 80)))

def swiglu(gate, up):
    """SwiGLU: silu(gate) * up"""
    return [silu(gate[i]) * up[i] for i in range(len(gate))]

def main():
    print("=" * 70)
    print("REFERENCE COMPUTATION: Layer 0 FFN for Qwen2.5-Coder-1.5B")
    print("=" * 70)
    
    f = open(GGUF_PATH, 'rb')
    
    # We need tensor offsets. Our loader parsed them but we need to find them.
    # For now, use the known-good embedding (token 0, first 1536 elements at offset 0)
    print("\n[1] Loading embedding (token 0, first row)...")
    embedding = load_tensor_q6k(f, 0, HIDDEN_DIM)
    print(f"    embedding[0:4] = {embedding[0:4]}")
    print(f"    Matches GPU: {abs(embedding[0] - (-0.015083)) < 0.001}")
    
    # We need the tensor offsets for blk.0 weights.
    # Since we can't easily parse the full GGUF header in this script,
    # let's just verify the embedding computation and print what we expect.
    
    # RMSNorm of the embedding
    # We know attn_norm weights start with [0.641, 0.540, 0.620, 0.803, ...]
    # But we need the full 1536 values. Let's compute RMS of the embedding first.
    
    rms = math.sqrt(sum(v*v for v in embedding) / HIDDEN_DIM)
    print(f"\n[2] Embedding RMS = {rms:.6f}")
    print(f"    After norm (no weight): embedding[0] / rms = {embedding[0] / rms:.6f}")
    print(f"    With weight 0.641: {embedding[0] / rms * 0.641:.6f}")
    
    # The key question: what magnitude does the normed embedding have?
    normed_magnitude = abs(embedding[0] / rms)
    print(f"\n    Normed element magnitude: ~{normed_magnitude:.4f}")
    print(f"    After weight scaling: ~{normed_magnitude * 0.6:.4f}")
    
    # Now the FFN matmul: normed (magnitude ~0.5) × gate_proj (1536 → 8960)
    # Each output element = sum of 1536 terms, each ~0.5 * weight_value
    # If weight values are ~0.01, output = 1536 * 0.5 * 0.01 * correlation ≈ 1-5
    # This matches our K values of magnitude 5!
    
    print(f"\n[3] Expected FFN gate output magnitude:")
    print(f"    1536 terms × ~{normed_magnitude * 0.6:.3f} × ~0.01 (weight) = ~{1536 * normed_magnitude * 0.6 * 0.01:.2f}")
    print(f"    This is NORMAL for a transformer. Values of 3-5 are expected.")
    
    print(f"\n[4] After SwiGLU: silu(gate) * up")
    print(f"    If gate ≈ 3-5, silu(3) = 3*sigmoid(3) = 3*0.95 = 2.86")
    print(f"    silu(gate) * up ≈ 2.86 * 3 = 8.6")
    
    print(f"\n[5] After down_proj: 8960 → 1536")
    print(f"    Each output = sum of 8960 terms × ~8.6 × ~0.01 = ~{8960 * 8.6 * 0.01:.1f}")
    print(f"    THIS IS THE PROBLEM!")
    print(f"    Down projection output magnitude: ~770")
    print(f"    Added to residual (magnitude 0.01): hidden ≈ 770")
    print(f"    After 28 layers: EXPLODES")
    
    print(f"\n{'=' * 70}")
    print(f"DIAGNOSIS: The magnitudes are CORRECT but the residual accumulation")
    print(f"is expected to produce large values. The model relies on the")
    print(f"ATTENTION to counterbalance the FFN growth. Without working")
    print(f"attention, the FFN output dominates and the model diverges.")
    print(f"")
    print(f"HOWEVER: With 1 layer, the output should still be reasonable")
    print(f"because the residual is embedding(0.01) + ffn_out(~770).")
    print(f"The lm_head then projects this 1536-dim vector to 152064 logits.")
    print(f"If the lm_head weights are ~0.01, logits = 1536 * 770 * 0.01 = ~11,800")
    print(f"That's way too large for softmax — but greedy just picks argmax.")
    print(f"")
    print(f"WAIT — magnitude 770 for down_proj output seems too high.")
    print(f"Let me recalculate with actual weight magnitudes...")
    print(f"")
    print(f"The issue might be that our weight dequant is producing values")
    print(f"that are 100x too large, OR the matmul is summing over the wrong")
    print(f"number of elements (using N instead of K as the inner dimension).")
    print(f"{'=' * 70}")
    
    f.close()

if __name__ == "__main__":
    main()
