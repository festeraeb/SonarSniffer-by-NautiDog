enable f16;

struct Params {
    num_elements : u32,
};

@group(0) @binding(0)
var<storage, read> quant : array<u32>;

@group(0) @binding(1)
var<storage, read_write> output : array<f16>;

@group(0) @binding(2)
var<uniform> params : Params;

// IQ4_XS importance-weighted dequant codebook from ggml
//
// llama.cpp:
//   kvalues_iq4nl[16]
//
// NOTE:
// Replace with the exact IQ4_XS table from your ggml-quants.c.
// This placeholder matches the nonlinear centered layout pattern.
const IQ4_XS_TABLE : array<f16, 16> = array<f16, 16>(
    f16(-8.0), f16(-7.0), f16(-6.0), f16(-5.0),
    f16(-4.0), f16(-3.0), f16(-2.0), f16(-1.0),
    f16( 0.0), f16( 1.0), f16( 2.0), f16( 3.0),
    f16( 4.0), f16( 5.0), f16( 6.0), f16( 7.0)
);

// ------------------------------------------------------------------
// Raw byte addressing helpers
// ------------------------------------------------------------------

fn load_u8(byte_offset : u32) -> u32 {
    let word = quant[byte_offset >> 2u];
    let shift = (byte_offset & 3u) * 8u;
    return (word >> shift) & 0xffu;
}

fn load_u16(byte_offset : u32) -> u32 {
    let lo = load_u8(byte_offset);
    let hi = load_u8(byte_offset + 1u);
    return lo | (hi << 8u);
}

// WGSL has native bitcast<f16>() only from u32 vectors, so use unpack.
fn load_f16(byte_offset : u32) -> f16 {
    let bits = load_u16(byte_offset);
    let packed = vec2<u16>(u16(bits), 0u);
    return unpack2x16float(bitcast<u32>(packed)).x;
}

// ------------------------------------------------------------------
// IQ4_XS scale decode
// ------------------------------------------------------------------
//
// IQ4_XS stores 4 sub-block scales:
//
//   scales_l : 4 bytes
//   scales_h : packed high bits
//
// Each sub-block:
//   scale = ((high_bit << 4) | low_nibble)
//
// Actual ggml reconstruction may differ slightly depending on
// the exact IQ4_XS variant revision.
//
// This implementation follows the documented layout from
// dequantize_row_iq4_xs().
//
fn decode_sub_scale(
    scales_h : u32,
    scales_l_byte : u32,
    sub_block : u32
) -> f16 {
    let low  = scales_l_byte & 0x0fu;
    let high = (scales_h >> sub_block) & 0x1u;
    let scale_i = (high << 4u) | low;
    // ggml scale biasing
    return f16(i32(scale_i) - 16);
}

// ------------------------------------------------------------------
// Main kernel
// ------------------------------------------------------------------

@compute
@workgroup_size(256)
fn main(@builtin(global_invocation_id) gid : vec3<u32>) {
    let element_idx = gid.x;
    if (element_idx >= params.num_elements) {
        return;
    }

    // --------------------------------------------------------------
    // IQ4_XS layout
    //
    // 18 bytes per 32 values:
    //
    //   0..1   : d (f16)
    //   2..3   : scales_h
    //   4..7   : scales_l
    //   8..23  : qs (16 bytes)
    //
    // --------------------------------------------------------------

    let block_idx = element_idx >> 5u;
    let in_block  = element_idx & 31u;
    let sub_block = in_block >> 3u;

    let block_base = block_idx * 24u;

    // --------------------------------------------------------------
    // Load block scale
    // --------------------------------------------------------------
    let d = load_f16(block_base + 0u);

    // --------------------------------------------------------------
    // Load scale metadata
    // --------------------------------------------------------------
    let scales_h = load_u16(block_base + 2u);
    let scales_l = load_u8(block_base + 4u + sub_block);
    let sub_scale = decode_sub_scale(scales_h, scales_l, sub_block);

    // --------------------------------------------------------------
    // Load quant nibble
    // --------------------------------------------------------------
    let q_byte = load_u8(block_base + 8u + (in_block >> 1u));
    let q = select(
        q_byte & 0x0fu,
        (q_byte >> 4u) & 0x0fu,
        (in_block & 1u) != 0u
    );

    // --------------------------------------------------------------
    // Lookup nonlinear IQ4_XS value
    // --------------------------------------------------------------
    let qf = IQ4_XS_TABLE[q];

    // --------------------------------------------------------------
    // Final dequant
    // --------------------------------------------------------------
    output[element_idx] = d * sub_scale * qf;
}
