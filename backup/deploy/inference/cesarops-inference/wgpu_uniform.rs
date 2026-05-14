// src/wgpu_uniform.rs
// Dynamic uniform buffer for matmul_half2.wgsl shape dimensions.
// Binding 3 in the shader expects this exact layout.
//
// NOTE: Requires wgpu crate. Currently a standalone module.
// Will be wired once candle-core is patched into workspace.

/// Tightly aligned structure mirroring the uniform memory layout
/// expected by matmul_half2.wgsl compute shader.
#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MatrixDimensions {
    pub m: u32,
    pub k: u32,
    pub n: u32,
    pub pad: u32, // 16-byte alignment padding
}

impl MatrixDimensions {
    pub fn new(m: usize, k: usize, n: usize) -> Self {
        Self {
            m: m as u32,
            k: k as u32,
            n: n as u32,
            pad: 0,
        }
    }

    /// Get as raw bytes for GPU upload
    pub fn as_bytes(&self) -> &[u8] {
        bytemuck::bytes_of(self)
    }
}
