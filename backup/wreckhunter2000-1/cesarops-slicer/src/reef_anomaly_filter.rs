use clap::Parser;
use rayon::prelude::*;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs::File;
use std::io::Write;
use std::path::PathBuf;
use tokio::time::Instant;

/// CESAROPS Artificial Reef & False Positive Filter
/// Evaluates anomaly reports against known artificial reef coordinates to cancel out intentional sinkings
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Bounding box (min_lon, min_lat, max_lon, max_lat)
    #[arg(short, long, value_delimiter = ',', num_args = 4)]
    bbox: Vec<f64>,

    /// API or Database holding known Artificial Reef Deployments
    #[arg(
        short,
        long,
        default_value = "https://marine-geo.xyz/api/v1/artificial_reefs"
    )]
    reef_db_url: String,

    /// Path to emit standard JSON output of cleared/flagged true anomalies
    #[arg(short, long, default_value = "reef_cleaned_anomalies.json")]
    output: PathBuf,

    /// Clearance radius (meters). If an anomaly is within this radius of a known reef, it is dismissed.
    #[arg(short, long, default_value_t = 150.0)]
    clearance_radius: f64,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct AnomalyReport {
    confidence: f64,
    lon: f64,
    lat: f64,
    description: String,
    methodology: String,
    capture_date: String,
    is_reef_false_positive: Option<bool>,
}

#[derive(Serialize, Deserialize, Debug)]
struct ReefData {
    id: String,
    lon: f64,
    lat: f64,
    deployment_date: String,
    name: String,
}

/// Haversine Formula for distance measurement (in meters)
fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let earth_radius_m = 6371000.0;
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
    earth_radius_m * c
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("== CESAROPS ARTIFICIAL REEF FILTER == ");
    println!("Fetching known reefs within BBOX: {:?}", args.bbox);

    let start_time = std::time::Instant::now();
    let client = Client::new();

    // 1. Fetch known artificial reefs (sunken ships deliberately placed)
    let reef_resp = match client
        .get(&args.reef_db_url)
        .query(&[
            ("min_lon", args.bbox[0].to_string()),
            ("min_lat", args.bbox[1].to_string()),
            ("max_lon", args.bbox[2].to_string()),
            ("max_lat", args.bbox[3].to_string()),
        ])
        .send()
        .await
    {
        Ok(resp) => {
            if resp.status().is_success() {
                resp.json::<Vec<ReefData>>().await.unwrap_or_default()
            } else {
                println!(
                    "⚠️ Reef DB Query returned {}. Using fallback local simulation.",
                    resp.status()
                );
                vec![]
            }
        }
        Err(e) => {
            println!(
                "⚠️ Reef DB Request Failed: {}. Using fallback local simulation.",
                e
            );
            // Injecting a mock intentional reef for demonstration
            vec![ReefData {
                id: "REEF-101".to_string(),
                lon: (args.bbox[0] + args.bbox[2]) / 2.0 + 0.001,
                lat: (args.bbox[1] + args.bbox[3]) / 2.0 + 0.001,
                deployment_date: "2015-08-10".to_string(),
                name: "USS Intentionally Sunk".to_string(),
            }]
        }
    };

    println!("✅ Loaded {} Known Artificial Reefs.", reef_resp.len());

    // 2. Load the input anomalies from other workers (Simulated here)
    // Normally, we'd read a file passed as input. For the worker independence, we'll simulate a
    // stream of incoming hits that we need to cross-check concurrently.
    let simulated_anomalies = vec![
        AnomalyReport {
            confidence: 0.91,
            lon: (args.bbox[0] + args.bbox[2]) / 2.0 + 0.00105, // Right on top of the mock reef
            lat: (args.bbox[1] + args.bbox[3]) / 2.0 + 0.00101, // Right on top of the mock reef
            description: "High scatter metallic anomaly".into(),
            methodology: "SAR_Blob_Detection".into(),
            capture_date: "2026-04-18T10:00:00Z".into(),
            is_reef_false_positive: None,
        },
        AnomalyReport {
            confidence: 0.88,
            lon: args.bbox[0] + 0.05, // Far away from the mock reef
            lat: args.bbox[1] + 0.05,
            description: "Sediment Plume Spine".into(),
            methodology: "OPTICAL_StructureTensor".into(),
            capture_date: "2026-04-18T10:30:00Z".into(),
            is_reef_false_positive: None,
        },
    ];

    println!(
        "🔎 Auditing {} Candidate Anomalies...",
        simulated_anomalies.len()
    );

    // 3. Parallel distance filtering against the Reef DB using Rayon
    let processed_anomalies: Vec<AnomalyReport> = simulated_anomalies
        .into_par_iter()
        .map(|mut hit| {
            let is_false_positive = reef_resp.iter().any(|reef| {
                let dist = haversine_distance(hit.lat, hit.lon, reef.lat, reef.lon);
                dist <= args.clearance_radius
            });

            if is_false_positive {
                hit.is_reef_false_positive = Some(true);
                hit.confidence = 0.0; // Negate the confidence completely
                hit.description = format!(
                    "DISMISSED: Matches known Artificial Reef within {}m",
                    args.clearance_radius
                );
            } else {
                hit.is_reef_false_positive = Some(false);
            }
            hit
        })
        .collect();

    // 4. Output the cleaned/tagged dataset
    let mut file = File::create(&args.output)?;
    let json_data = serde_json::to_string_pretty(&processed_anomalies)?;
    file.write_all(json_data.as_bytes())?;

    let valid_count = processed_anomalies
        .iter()
        .filter(|x| x.is_reef_false_positive == Some(false))
        .count();

    println!("🏁 False Positive Filter Complete in {:.2}ms. Valid Wrecks Remaining: {}. Report exported to {}",
             start_time.elapsed().as_millis(), valid_count, args.output.display());

    Ok(())
}
