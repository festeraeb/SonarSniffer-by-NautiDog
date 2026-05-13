//! # CESAROPS Unified Slicer
//!
//! Zero-copy, memory-mapped GeoTIFF tile extractor with coordinate baking.
//!
//! Takes satellite data from any provider (Sentinel, Landsat, Maxar, Planet)
//! and slices it into GPU/TPU-ready tiles with anchored coordinates
//! pre-baked into each tile's sidecar JSON.
//!
//! ## Architecture
//!
//! ```text
//! GeoTIFF(s) → memmap2 Virtual Mapping → Tile Slicer → [Tile Pixels + Anchor JSON] → GPU/TPU
//! ```
//!
//! ## Key Concepts
//!
//! - **Zero-Copy**: Uses `memmap2` to treat GeoTIFF files as virtual memory arrays.
//!   No full-file loads, no 4GB BigTIFF crashes on 8GB RAM rigs.
//! - **Coordinate Baking**: Each tile's top-left lat/lon is calculated ONCE during
//!   slicing and stored in a sidecar JSON. Scanner adds local pixel offset to
//!   tile origin for zero-drift positioning.
//! - **Multi-Source VRT**: Multiple GeoTIFFs from different providers are wrapped
//!   into a unified virtual dataset with normalized metadata.

pub mod common;
pub mod io;
pub mod spec;
pub mod tiles;

pub use common::db::{AnomalyQueue, AnomalyRecord, ReviewStatus, SensorType};
pub use io::gdal_warp::{check_gpu_warp_available, gpu_warp, WarpResult};
pub use io::geotiff::MmapGeoTiff;
pub use io::vrt::{ResampleMethod, VrtDataset, VrtNormalizer, VrtSource};
pub use io::watcher::PreprocessWatcher;
pub use spec::delegate::DelegateTarget;
pub use spec::mission::{MissionSpec, ModuleSpec, OutputStrategy};
pub use tiles::anchor::{AnchorCalculator, TileAnchor};
pub use tiles::slicer::TileSlicer;
pub use tiles::vrt_slicer::{
    build_vrt_from_mission, DriftStats, TileDrift, TileOverlap, VrtQualityReport, VrtSlicedTile,
    VrtTileManifest, VrtTileSlicer,
};
