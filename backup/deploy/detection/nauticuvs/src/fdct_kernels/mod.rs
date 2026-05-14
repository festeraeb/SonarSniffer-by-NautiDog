//! FDCT inner-loop kernel functions.
//!
//! All functions operate on flat, contiguous slices with no heap allocation
//! in the hot path. When the `xla` Cargo feature is active, each function is
//! annotated with `#[xla::kernel]` for TPU compilation.

pub mod tile;
pub mod window;
pub mod wrap;

pub use window::WedgeParams;
