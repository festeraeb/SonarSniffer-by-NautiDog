// WGSL Q6_K Dequantization — Flat Buffer Access (no struct alignment issues)
//
// Q6_K super-block: 210 bytes = 256 elements
// Byte layout (GGUF native order):
//   [0..127]   ql: 128 bytes — lower 4 bits per element (2 per byte)
//   [128..191] qh: 64 bytes — upper 2 bits per element (4 per byte)
//   [192..207] scales: 16 bytes — 16 signed i8 sub-block scales
//   [208..209] d: 2 bytes — f16 global scale
//
// We read the raw buffer as array<u32> and manually extract bytes.
// Block size in u32 words: ceil(210/4) = 53 words (with 2 bytes padding at end)
// Actually: 210 bytes = 52 full u32s + 2 remaining bytes. We'll use 53 u32s per block
// and mask the last partial word.

struct Params {
    total_blocks: u32,
    _pad0: u32,
    _pad1: u32,
    _pad2: u32,
}

@group(0) @binding(0) var<storage, read> raw_data: array<u32>;
@group(0) @binding(1) var<storage, read_write> output_f32: array<f32>;
@group(0) @binding(2) var<uniform> params: Params;

fn fp16_to_f32(bits: u32) -> f32 {
    let s = (bits >> 15u) & 0x1u;
    let e = (bits >> 10u) & 0x1fu;
    let m = bits & 0x3ffu;
    if (e == 0u) {
        if (m == 0u) { return 0.0; }
        return select(-1.0, 1.0, s == 0u) * f32(m) * 5.96046447e-8;
    }
    if (e == 31u) { return select(-1.0, 1.0, s == 0u) * 65504.0; }
    return select(-1.0, 1.0, s == 0u) * pow(2.0, f32(e) - 15.0) * (1.0 + f32(m) / 1024.0);
}

// Read a single byte from the raw u32 array at a given byte offset within a block
fn read_byte(block_byte_offset: u32, local_byte: u32) -> u32 {
    let abs_byte = block_byte_offset + local_byte;
    let word_idx = abs_byte / 4u;
    let byte_in_word = abs_byte % 4u;
    return (raw_data[word_idx] >> (byte_in_word * 8u)) & 0xFFu;
}

@compute @workgroup_size(256, 1, 1)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let global_elem = gid.x;
    let block_idx = global_elem / 256u;
    let local_idx = global_elem % 256u;

    if (block_idx >= params.total_blocks) { return; }

    // Byte offset of this block in the raw buffer
    let block_byte_offset = block_idx * 210u;

    // Read global scale d (bytes 208-209, f16)
    let d_lo = read_byte(block_byte_offset, 208u);
    let d_hi = read_byte(block_byte_offset, 209u);
    let d = fp16_to_f32(d_lo | (d_hi << 8u));

    // Read sub-block scale (bytes 192-207, 16 signed i8 values)
    let sub_idx = local_idx / 16u;
    let scale_byte = read_byte(block_byte_offset, 192u + sub_idx);
    // Interpret as signed i8: if >= 128, subtract 256
    let scale_signed = i32(scale_byte) - select(0, 256, scale_byte >= 128u);

    // Read lower 4 bits from ql region (bytes 0-127, 2 nibbles per byte)
    let ql_byte_idx = local_idx / 2u;
    let ql_byte = read_byte(block_byte_offset, ql_byte_idx);
    var ql_val: u32;
    if (local_idx % 2u == 0u) {
        ql_val = ql_byte & 0x0Fu;
    } else {
        ql_val = (ql_byte >> 4u) & 0x0Fu;
    }

    // Read upper 2 bits from qh region (bytes 128-191, 4 crumbs per byte)
    let qh_byte_idx = local_idx / 4u;
    let qh_byte = read_byte(block_byte_offset, 128u + qh_byte_idx);
    let qh_shift = (local_idx % 4u) * 2u;
    let qh_val = (qh_byte >> qh_shift) & 0x03u;

    // Reconstruct 6-bit signed value: (qh << 4) | ql, centered at 32
    let q6 = i32(ql_val | (qh_val << 4u)) - 32;

    // Final dequantized weight
    output_f32[global_elem] = d * f32(scale_signed) * f32(q6);
}
