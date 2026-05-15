//! Dynamic dequantization probe — validates shader bit-layout against known values.
//!
//! Creates a synthetic Q6_K block with predictable values, runs it through the
//! GPU dequant shader, reads back the results, and verifies correctness.
//! If the output doesn't match expected values, reports the exact discrepancy
//! so we can fix the shader.

use bytemuck::{Pod, Zeroable};
use crate::shader_ops::DequantQ6KPipeline;
use tracing::{info, warn, error};

/// Result of the dequant probe.
#[derive(Debug)]
pub struct ProbeResult {
    pub passed: bool,
    pub expected: Vec<f32>,
    pub actual: Vec<f32>,
    pub max_error: f32,
}

/// Run the Q6_K dequant probe to validate shader correctness.
///
/// Creates a synthetic block where:
/// - d (global scale) = 1.0 (f16: 0x3C00)
/// - scales[0] = 32 + 1 = 33 (so signed scale = 33 - 32 = 1, but we store raw i8)
///   Actually: scales are i8 stored as u8. We'll use scale = 1 (stored as 1)
///   Wait — in our shader, scale = i32(byte) - 32. So to get scale=1, store 33.
///   Hmm, let me re-check. In the shader: `let scale_signed = i32(scale_byte) - select(0, 256, scale_byte >= 128u)`
///   That's treating it as unsigned with manual sign extension. So byte 33 → i32(33) = 33.
///   But in the GGUF spec, Q6_K scales are plain i8 (signed). So byte 0xFF = -1, byte 0x01 = 1.
///   Our shader does: i32(byte) - select(0, 256, byte >= 128). So 0xFF → 255 - 256 = -1. Correct.
///   And 0x01 → 1 - 0 = 1. Correct.
///
/// For the probe:
/// - d = 1.0 (f16 bits: 0x3C00)
/// - scales[0] = 1 (raw byte 0x01, shader interprets as 1)
/// - ql nibbles all = 5 (so ql bytes = 0x55)
/// - qh crumbs all = 1 (so qh bytes = 0x55 → bits 01 01 01 01)
///
/// Expected reconstruction:
///   q6 = (qh << 4) | ql = (1 << 4) | 5 = 21
///   quantized = 21 - 32 = -11
///   weight = d * scale * quantized = 1.0 * 1 * (-11) = -11.0
pub fn run_dequant_probe(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    dequant_pipeline: &DequantQ6KPipeline,
) -> ProbeResult {
    info!("Running Q6_K dequant probe...");

    // Build a synthetic 210-byte Q6_K block
    let mut probe_bytes = vec![0u8; 212]; // 210 + 2 padding to align to 4 bytes

    // ql region (bytes 0-127): all nibbles = 5 → byte = 0x55
    for i in 0..128 {
        probe_bytes[i] = 0x55;
    }

    // qh region (bytes 128-191): all crumbs = 1 → byte = 0b01010101 = 0x55
    for i in 128..192 {
        probe_bytes[i] = 0x55;
    }

    // scales region (bytes 192-207): all = 1
    for i in 192..208 {
        probe_bytes[i] = 1;
    }

    // d region (bytes 208-209): f16 = 1.0 = 0x3C00
    probe_bytes[208] = 0x00; // low byte
    probe_bytes[209] = 0x3C; // high byte

    // Upload to GPU as raw u32 array
    let input_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe_input"),
        size: 212, // Padded to 4-byte alignment
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    queue.write_buffer(&input_buf, 0, &probe_bytes);

    // Output buffer: 256 f32 elements
    let output_buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe_output"),
        size: 256 * 4,
        usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
        mapped_at_creation: false,
    });

    // Dispatch dequant
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("probe_encoder"),
    });
    dequant_pipeline.dispatch(device, queue, &mut encoder, &input_buf, &output_buf, 256);

    // Readback
    let staging = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("probe_staging"),
        size: 256 * 4,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    encoder.copy_buffer_to_buffer(&output_buf, 0, &staging, 0, 256 * 4);
    queue.submit(std::iter::once(encoder.finish()));

    // Map and read
    let slice = staging.slice(..);
    let (tx, rx) = std::sync::mpsc::channel();
    slice.map_async(wgpu::MapMode::Read, move |r| { let _ = tx.send(r); });
    loop {
        device.poll(wgpu::Maintain::Poll);
        if rx.try_recv().is_ok() { break; }
        std::thread::sleep(std::time::Duration::from_micros(10));
    }
    let data = slice.get_mapped_range();
    let actual: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
    drop(data);
    staging.unmap();

    // Expected: all elements should be d * scale * (q6 - 32)
    // d = 1.0, scale = 1 (raw byte 1, shader does i32(1) - select(0,256,1>=128) = 1)
    // ql = 5, qh = 1, q6 = (1<<4)|5 = 21, quantized = 21 - 32 = -11
    // weight = 1.0 * 1 * (-11) = -11.0
    let expected_val = -11.0f32;
    let expected: Vec<f32> = vec![expected_val; 256];

    let max_error = actual.iter().zip(expected.iter())
        .map(|(a, e)| (a - e).abs())
        .fold(0.0f32, f32::max);

    let passed = max_error < 0.01;

    if passed {
        info!("✓ Dequant probe PASSED — all 256 elements match expected value ({:.1})", expected_val);
    } else {
        error!("✗ Dequant probe FAILED — max error: {:.4}", max_error);
        info!("  Expected[0..4]: {:?}", &expected[0..4]);
        info!("  Actual[0..4]:   {:?}", &actual[0..4]);
        info!("  Actual[0..16]:  {:?}", &actual[0..16]);

        // Diagnostic: check if values suggest byte-swap
        if actual[0].abs() > 1000.0 || actual[0].is_nan() {
            warn!("  Values suggest byte-endianness mismatch in shader");
        } else if (actual[0] - expected_val).abs() < 100.0 {
            warn!("  Values are in reasonable range but wrong — likely bit-shift offset error");
            // Try to reverse-engineer what the shader actually computed
            if actual[0] != 0.0 {
                let ratio = actual[0] / expected_val;
                info!("  Ratio actual/expected = {:.4} (scale factor off by this amount)", ratio);
            }
        }
    }

    ProbeResult { passed, expected, actual, max_error }
}
