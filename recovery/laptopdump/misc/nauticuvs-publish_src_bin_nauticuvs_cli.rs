use clap::Parser;
use ndarray::Array2;
use ndarray_npy::read_npy;
use std::fs::File;
use nauticuvs::{curvelet_forward_config, curvelet_inverse, CurveletConfig};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    input: String,
    #[arg(long, default_value_t = 5)]
    scales: usize,
    #[arg(long, default_value_t = 32)]
    directions: usize,
    #[arg(long, default_value_t = false)]
    thresholding: bool,
    #[arg(long, default_value_t = 0.05)]
    threshold: f64,
}

fn main() {
    let args = Args::parse();
    let file = File::open(&args.input).expect("Failed to open input file");
    let arr: Array2<f32> = read_npy(file).expect("Failed to read npy");

    let config = CurveletConfig::new(args.scales)
        .expect("Invalid scales")
        .with_finest_directions(args.directions)
        .expect("Invalid direction count");
    let mut coeffs = curvelet_forward_config(&arr, &config).expect("Curvelet forward failure");

    if args.thresholding {
        coeffs.hard_threshold(args.threshold);
    }

    let rec = curvelet_inverse(&coeffs).expect("Curvelet inverse failure");

    // measure energy per direction on scale=3
    let scale_idx = 3.min(coeffs.detail.len().saturating_sub(1));
    let scale = &coeffs.detail[scale_idx];
    let energies: Vec<f64> = scale.iter().map(|dir| dir.mapv(|c| (c.norm_sqr() as f64)).sum()).collect();
    let e_total: f64 = energies.iter().sum();

    println!("recon_mean={}, recon_std={}", rec.mean().unwrap(), rec.std(0.0));
    for (i, e) in energies.iter().enumerate() {
        println!("direction_{}={}", i, e / (e_total + 1e-12));
    }
}
