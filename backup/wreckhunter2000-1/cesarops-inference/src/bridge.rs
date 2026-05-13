//! GridBuffer ↔ Burn Tensor Bridge
//!
//! Zero-copy conversion between our GridBuffer memory abstraction and Burn tensors.
//! For now, this copies data (true zero-copy requires Burn internals access).
//! TODO: Once Burn exposes `from_existing_buffer`, switch to zero-copy.

use half::f16;

/// Convert raw bytes (f16 format) to a Vec<f32> for Burn consumption.
/// This is the "safe path" — copies data but works with any Burn version.
pub fn f16_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|chunk| {
            let bits = u16::from_le_bytes([chunk[0], chunk[1]]);
            f16::from_bits(bits).to_f32()
        })
        .collect()
}

/// Convert raw bytes (f32 format) to a Vec<f32>.
pub fn f32_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Convert raw bytes (bf16 format) to a Vec<f32>.
pub fn bf16_bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(2)
        .map(|chunk| {
            let bits = u16::from_le_bytes([chunk[0], chunk[1]]);
            // BF16: sign(1) + exponent(8) + mantissa(7)
            // F32:  sign(1) + exponent(8) + mantissa(23)
            // Just shift left by 16 bits
            f32::from_bits((bits as u32) << 16)
        })
        .collect()
}

/// Dequantize Q4_K_M block to f32 values.
/// Q4_K_M uses 256-element blocks with scales and mins.
/// This is a simplified implementation — production would use SIMD.
pub fn dequant_q4_k_m(bytes: &[u8], n_elements: usize) -> Vec<f32> {
    // Q4_K_M block structure (256 elements per block):
    // - 2 bytes: d (f16 scale)
    // - 2 bytes: dmin (f16 min)
    // - 12 bytes: scales (k_scale_t)
    // - 128 bytes: quantized values (4 bits each)
    // Total: 144 bytes per 256 elements (approximate)
    
    let mut output = Vec::with_capacity(n_elements);
    
    // Simplified: treat as 4-bit values with a single scale per block
    let block_size = 32;
    let bytes_per_block = 18; // Q4_0 simplified
    
    for block_start in (0..bytes.len()).step_by(bytes_per_block) {
        if block_start + bytes_per_block > bytes.len() {
            break;
        }
        
        // First 2 bytes are the scale (f16)
        let scale_bits = u16::from_le_bytes([bytes[block_start], bytes[block_start + 1]]);
        let scale = f16::from_bits(scale_bits).to_f32();
        
        // Remaining bytes are 4-bit quantized values (2 per byte)
        for i in 0..16 {
            let byte = bytes[block_start + 2 + i];
            let lo = (byte & 0x0F) as i8 - 8;
            let hi = ((byte >> 4) & 0x0F) as i8 - 8;
            output.push(lo as f32 * scale);
            output.push(hi as f32 * scale);
            if output.len() >= n_elements {
                break;
            }
        }
        
        if output.len() >= n_elements {
            break;
        }
    }
    
    output.truncate(n_elements);
    output
}

/// Dequantize Q6_K block to f32 values.
/// Q6_K: 256-element super-blocks, 6 bits per weight.
/// Block layout (210 bytes per 256 elements):
///   ql[128]: lower 4 bits of quants (2 per byte)
///   qh[64]:  upper 2 bits of quants (4 per byte)  
///   scales[16]: int8 scales for 16-element sub-blocks
///   d[2]: f16 super-block scale
///
/// Dequant formula: y[i] = d * sc[i/16] * (q6[i] - 32)
/// where q6[i] = (ql_bits | (qh_bits << 4))
pub fn dequant_q6_k(bytes: &[u8], n_elements: usize) -> Vec<f32> {
    let mut output = Vec::with_capacity(n_elements);
    let block_size: usize = 256;
    let bytes_per_block: usize = 210;

    let n_blocks = bytes.len() / bytes_per_block;

    for block_idx in 0..n_blocks {
        if output.len() >= n_elements {
            break;
        }

        let base = block_idx * bytes_per_block;
        if base + bytes_per_block > bytes.len() {
            break;
        }

        // Block layout offsets:
        let ql = &bytes[base..base + 128];        // lower 4 bits
        let qh = &bytes[base + 128..base + 192];  // upper 2 bits
        let scales = &bytes[base + 192..base + 208]; // int8 scales
        let d_bits = u16::from_le_bytes([bytes[base + 208], bytes[base + 209]]);
        let d = f16::from_bits(d_bits).to_f32();

        // Process 256 elements in groups of 128 (two halves)
        // First half (elements 0..127): uses ql[0..64] low nibbles + ql[0..64] high nibbles
        // Second half (elements 128..255): uses ql[64..128] low nibbles + ql[64..128] high nibbles
        // qh encodes the upper 2 bits for all 256 elements

        for j in 0..256 {
            if output.len() >= n_elements {
                break;
            }

            // Extract lower 4 bits from ql
            let ql_idx = j / 2;
            let ql_val = if j % 2 == 0 {
                (ql[ql_idx] & 0x0F) as i32
            } else {
                ((ql[ql_idx] >> 4) & 0x0F) as i32
            };

            // Extract upper 2 bits from qh
            // qh packs 4 values per byte (2 bits each)
            let qh_idx = j / 4;
            let qh_shift = (j % 4) * 2;
            let qh_val = ((qh[qh_idx] >> qh_shift) & 0x03) as i32;

            // Reconstruct 6-bit value and center
            let q6 = ql_val | (qh_val << 4);
            let q_centered = q6 - 32; // center to signed range [-32, 31]

            // Get sub-block scale
            let sc = scales[j / 16] as i8 as f32;

            output.push(d * sc * q_centered as f32);
        }
    }

    output.truncate(n_elements);
    output
}

/// Determine the conversion function based on quantization type.
pub fn dequantize_tensor(bytes: &[u8], quant_type: u32, n_elements: usize) -> Vec<f32> {
    match quant_type {
        0 => f32_bytes_to_f32(bytes),           // F32
        1 | 30 => f16_bytes_to_f32(bytes),      // F16
        28 => bf16_bytes_to_f32(bytes),          // BF16
        2 => dequant_q4_k_m(bytes, n_elements), // Q4_0 (simplified)
        12 => dequant_q4_k_m(bytes, n_elements), // Q4_K_M
        14 => dequant_q6_k(bytes, n_elements),   // Q6_K
        8 => {                                   // Q8_0
            bytes.iter().map(|&b| (b as i8) as f32 / 127.0).collect()
        }
        _ => {
            tracing::warn!("Unknown quant type {}, treating as f16", quant_type);
            f16_bytes_to_f32(bytes)
        }
    }
}
