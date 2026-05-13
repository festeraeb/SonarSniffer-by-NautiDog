//! Geographic coordinate and GeoTIFF ingestion modules.

pub mod geotiff;
pub mod transform;

pub use transform::GeoTransform;
pub use geotiff::GeoTiffInput;

/// Errors produced by GeoTIFF loading and coordinate reprojection.
#[derive(Debug, thiserror::Error)]
pub enum GeoTiffError {
    /// The GeoTIFF file does not contain a GeoTransform.
    #[error("Missing GeoTransform metadata in GeoTIFF file (tags 33550/33922/34264 not found)")]
    MissingGeoTransform,
    /// The TIFF data is corrupt or cannot be parsed.
    #[error("Corrupt or unreadable TIFF data: {0}")]
    CorruptData(String),
    /// The pixel format is not supported.
    #[error("Unsupported pixel format: {0}")]
    UnsupportedFormat(String),
    /// CRS reprojection failed.
    #[error("CRS reprojection error: {0}")]
    ReprojectError(String),
    /// File I/O error.
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
