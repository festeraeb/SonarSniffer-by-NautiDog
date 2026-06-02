//! CESAROPS BAG (bathymetry sonar) wreck + redaction detection library.
//!
//! This crate ports the NOAA BAG detection surface from the Python reference
//! pipeline (`pipelines/bag/`) and the Rust porting base
//! (`pipelines/bag/wreckhunter2000/src/bag_mesh.rs`) into a modular Rust crate.
//!
//! Module map:
//!   * [`types`]            — knobs, enums, output contract structs
//!   * [`grid`]             — shared grid helpers lifted from `bag_mesh.rs`
//!   * [`bag_io`]           — BAG dataset reader (gdal)
//!   * [`geo`]              — WGS84 reprojection (gdal OSR)
//!   * [`anomaly`]          — physical wreck (height-above-floor) detection
//!   * [`redaction_unmask`] — redaction/masking signature detection (marquee IP)
//!   * [`orientation`]      — PCA heading/length/width + compass bearing
//!   * [`dedup`]            — spatial deduplication
//!   * [`pipeline`]         — stage orchestration A->G

pub mod types;
pub mod grid;
pub mod bag_io;
pub mod geo;
pub mod anomaly;
pub mod redaction_unmask;
pub mod orientation;
pub mod dedup;
pub mod pipeline;
