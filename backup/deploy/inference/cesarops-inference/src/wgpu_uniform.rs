//! Uniform buffer struct for the matmul compute shader.
//! Must be 16-byte aligned and match the WGSL `MatrixDimensions` struct exactly.

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
pub struct MatrixDimensions {
    pub m: u32,
    pub k: u32,
    pub n: u32,
    pub pad: u32,
}
