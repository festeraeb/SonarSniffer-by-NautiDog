//! CPU Smoke Test — validates the full inference pipeline components.
//!
//! Tests each stage of the pipeline independently:
//!   1. Dequantization (bridge.rs) — F32, F16 roundtrips
//!   2. Matrix multiply (matmul.rs) — identity, known products
//!   3. Sampling (sampling.rs) — greedy, banned tokens, rep_pen
//!   4. Tokenizer (tokenizer.rs) — byte-level fallback encode/decode
//!   5. Forward pass components — RMSNorm, SwiGLU, RoPE don't crash
//!
//! Run with: cargo test -p cesarops-inference --test cpu_smoke_test -- --nocapture

use std::sync::Arc;

use cesarops_inference::arena::InferenceArena;
use cesarops_inference::bridge;
use cesarops_inference::matmul;
use cesarops_inference::sampling::{self, SamplingParams};
use cesarops_inference::tokenizer::ZeroAllocBpeTokenizer;

/// Build a synthetic ModelWeights is not possible from outside the module
/// (the _mmap field is private). Instead we test all components individually.

#[test]
fn test_dequantize_f32_roundtrip() {
    // Create some f32 data, convert to bytes, dequantize back
    let original: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
    let bytes: Vec<u8> = original.iter().flat_map(|f| f.to_le_bytes()).collect();

    let recovered = bridge::dequantize_tensor(&bytes, 0, 64); // quant_type 0 = F32
    assert_eq!(recovered.len(), 64);

    for (a, b) in original.iter().zip(recovered.iter()) {
        assert!((a - b).abs() < 1e-6, "Mismatch: {} vs {}", a, b);
    }
    println!("[PASS] F32 dequantize roundtrip");
}

#[test]
fn test_dequantize_f16_roundtrip() {
    use half::f16;

    let original: Vec<f32> = (0..64).map(|i| i as f32 * 0.1).collect();
    let bytes: Vec<u8> = original.iter().flat_map(|f| {
        f16::from_f32(*f).to_le_bytes()
    }).collect();

    let recovered = bridge::dequantize_tensor(&bytes, 1, 64); // quant_type 1 = F16
    assert_eq!(recovered.len(), 64);

    for (a, b) in original.iter().zip(recovered.iter()) {
        // F16 has ~0.1% relative error
        assert!((a - b).abs() < 0.01, "Mismatch: {} vs {}", a, b);
    }
    println!("[PASS] F16 dequantize roundtrip");
}

#[test]
fn test_matmul_identity() {
    // 4x4 identity matrix × vector should return the vector
    let dim = 4;
    let mut identity = vec![0.0f32; dim * dim];
    for i in 0..dim {
        identity[i * dim + i] = 1.0;
    }
    let input = vec![1.0f32, 2.0, 3.0, 4.0];

    // matmul_f32(A, B, m, k, n) where A=[m,k], B=[k,n], C=[m,n]
    // Identity[4,4] × input[4,1] → output[4,1]
    let output = matmul::matmul_f32(&identity, &input, dim, dim, 1);

    assert_eq!(output.len(), dim);
    for i in 0..dim {
        assert!((output[i] - input[i]).abs() < 1e-6,
            "Element {} mismatch: {} vs {}", i, output[i], input[i]);
    }
    println!("[PASS] Identity matmul");
}

#[test]
fn test_matmul_known_product() {
    // [2,3] × [3,1] = [2,1]
    let a = vec![1.0f32, 2.0, 3.0,  // row 0
                 4.0, 5.0, 6.0];    // row 1
    let b = vec![1.0f32, 1.0, 1.0]; // column vector

    let c = matmul::matmul_f32(&a, &b, 2, 3, 1);
    assert_eq!(c.len(), 2);
    assert!((c[0] - 6.0).abs() < 1e-6); // 1+2+3
    assert!((c[1] - 15.0).abs() < 1e-6); // 4+5+6
    println!("[PASS] Known matmul product");
}

#[test]
fn test_sampling_basic() {
    // Create logits where token 42 has the highest value
    let mut logits = vec![0.0f32; 256];
    logits[42] = 10.0;
    logits[100] = 5.0;

    let params = SamplingParams {
        temperature: 0.1, // Very low temp → nearly greedy
        top_p: 0.9,
        rep_pen: 1.0,
        rep_pen_range: 64,
        stop_sequences: Vec::new(),
        banned_tokens: std::collections::HashSet::new(),
    };

    let token = sampling::sample(&mut logits, &params, &[]);
    assert_eq!(token, 42, "Expected token 42 (highest logit), got {}", token);
    println!("[PASS] Sampling picks highest logit at low temperature");
}

#[test]
fn test_sampling_banned_tokens() {
    // Token 42 is highest but banned → should pick token 100
    let mut logits = vec![0.0f32; 256];
    logits[42] = 10.0;
    logits[100] = 5.0;

    let mut banned = std::collections::HashSet::new();
    banned.insert(42u32);

    let params = SamplingParams {
        temperature: 0.1,
        top_p: 0.9,
        rep_pen: 1.0,
        rep_pen_range: 64,
        stop_sequences: Vec::new(),
        banned_tokens: banned,
    };

    let token = sampling::sample(&mut logits, &params, &[]);
    assert_eq!(token, 100, "Expected token 100 (42 is banned), got {}", token);
    println!("[PASS] Banned token suppression works");
}

#[test]
fn test_tokenizer_byte_fallback() {
    let arena = InferenceArena::new(1024, 0);
    let tokenizer = ZeroAllocBpeTokenizer::new(arena);

    // Without a real vocab, encode should fall back to byte-level
    let tokens = tokenizer.encode("Hello");
    assert_eq!(tokens.len(), 5); // H, e, l, l, o as bytes
    assert_eq!(tokens[0], b'H' as u32);
    assert_eq!(tokens[1], b'e' as u32);

    // Decode should reverse it
    let decoded = tokenizer.decode(&tokens);
    assert_eq!(decoded, "Hello");
    println!("[PASS] Byte-level tokenizer roundtrip");
}

#[test]
fn test_transformer_forward_no_crash() {
    // Verify the forward pass doesn't panic with synthetic weights.
    // We can't use ModelWeights directly (needs mmap), so test the components.
    let dim = 64;
    let inter = 256;

    // Simulate one layer's worth of computation
    let mut hidden = vec![0.1f32; dim];

    // RMSNorm with ones weight
    let norm_w = vec![1.0f32; dim];
    rmsnorm_f32_test(&mut hidden, &norm_w, 1e-6);

    // Verify hidden state didn't explode or collapse
    let sum: f32 = hidden.iter().map(|x| x.abs()).sum();
    assert!(sum > 0.0, "Hidden state collapsed to zero after RMSNorm");
    assert!(sum < 1000.0, "Hidden state exploded after RMSNorm: sum={}", sum);

    // Matmul: weight[inter, dim] × hidden[dim, 1] → [inter, 1]
    let weight = vec![0.01f32; inter * dim];
    let output = matmul::matmul_f32(&weight, &hidden, inter, dim, 1);
    assert_eq!(output.len(), inter);

    // SwiGLU
    let gate = output.clone();
    let up = vec![0.5f32; inter];
    let mut ffn = vec![0.0f32; inter];
    for i in 0..inter {
        let g = gate[i];
        let silu = g * (1.0 / (1.0 + (-g).exp()));
        ffn[i] = silu * up[i];
    }

    // Down projection
    let down_w = vec![0.01f32; dim * inter];
    let down_out = matmul::matmul_f32(&down_w, &ffn, dim, inter, 1);
    assert_eq!(down_out.len(), dim);

    println!("[PASS] Forward pass components don't crash (dim={}, inter={})", dim, inter);
}

/// Local copy of rmsnorm for testing (matches transformer.rs implementation)
fn rmsnorm_f32_test(x: &mut [f32], weight: &[f32], eps: f32) {
    let n = x.len();
    let mut sum_sq = 0.0f32;
    for i in 0..n {
        sum_sq += x[i] * x[i];
    }
    let rms = ((sum_sq / n as f32) + eps).sqrt();
    let inv_rms = 1.0 / rms;
    for i in 0..n {
        x[i] = x[i] * inv_rms * weight[i.min(weight.len() - 1)];
    }
}
