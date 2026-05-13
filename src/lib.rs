pub mod config;
pub mod bands;
pub mod detect;

pub use config::MissionConfig;
pub use detect::Anomaly;

/// Unified pipeline entry point. Executes band math, normalizes, and runs detection.
pub async fn run_pipeline(tile_id: &str, config: &MissionConfig) -> Vec<Anomaly> {
    tracing::info!("Starting pipeline for tile: {}", tile_id);

    // Load bands (conceptual in-memory generation for compilation safety)
    let primary = bands::load_band("primary", config.band_recipe.primary.clone()).unwrap();
    let secondary = bands::load_band("secondary", config.band_recipe.secondary.clone()).unwrap();

    // Apply band recipe
    let processed = bands::apply_recipe(&primary, &secondary, &config.band_recipe).unwrap();

    // Run detection
    let anomalies = detect::detect_anomalies(tile_id, &processed, config);
    
    tracing::info!("Pipeline completed. Found {} anomalies.", anomalies.len());
    anomalies
}
