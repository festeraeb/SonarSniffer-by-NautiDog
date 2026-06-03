//! cesarops-satellite — unified satellite pipeline
//!
//! Modules map 1-to-1 with the Python pipeline files:
//!
//! | Python file                          | Rust module      |
//! |--------------------------------------|------------------|
//! | sat_mission_orchestrator.py          | mission          |
//! | wh2k_sentinel_wreck_targeting.py     | concept          |
//! | wh2k_sentinel_optical_poc.py         | poc              |
//! | sar_temporal_persistence.py          | sar              |
//! | nasa_fusion_test.py                  | nasa_fusion      |
//! | apply_opera_dswx.py                  | stac + downloads |
//! | temporal_stack_engine.py             | temporal         |
//! | wh2k_chip_extractor.py               | chip + magnetic  |
//! | wh2k_ab_attenuation.py               | spectral         |
//! | nasa_earthdata_client.py / stac      | stac             |
//! | historical_drift.py + buoy_analog.py | drift            |
//! | (new)                                | fusion           |

pub mod bathymetry_map;
pub mod buoy;
pub mod chip;
pub mod concept;
pub mod engine_error;
pub mod downloads;
pub mod drift;
pub mod env_conditions;
pub mod fusion;
pub mod lake_levels;
pub mod magnetic;
pub mod mission;
pub mod nasa_fusion;
pub mod overlay_grid;
pub mod phase_corr;
pub mod poc;
pub mod sar;
pub mod spectral;
pub mod stac;
pub mod temporal;
pub mod types;

pub use types::{
    BBox, Candidate, Knobs, MissionReport, MissionSpec, SarCluster, Stage, WreckTarget,
};
