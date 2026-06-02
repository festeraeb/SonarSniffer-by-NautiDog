//! Score a magnetic anomaly window with full-precision FDCT curvelet energy.
//! Usage: mag_curvelet_energy --npy /tmp/window.npy [--scales 5]

use clap::Parser;
use ndarray::Array2;
use ndarray_npy::ReadNpyExt;
use nauticuvs::curvelet_forward;
use std::fs::File;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "mag_curvelet_energy")]
struct Args {
    #[arg(long)]
    npy: PathBuf,
    #[arg(long, default_value_t = 5)]
    scales: usize,
}

fn main() {
    let args = Args::parse();
    let mut f = File::open(&args.npy).unwrap_or_else(|e| {
        eprintln!("open {}: {e}", args.npy.display());
        std::process::exit(1);
    });
    let arr: Array2<f32> = Array2::read_npy(&mut f).unwrap_or_else(|e| {
        eprintln!("read_npy: {e}");
        std::process::exit(1);
    });
    let coeffs = curvelet_forward(&arr, args.scales).unwrap_or_else(|e| {
        eprintln!("curvelet_forward: {e}");
        std::process::exit(1);
    });

    let mut ac_energy = 0.0f64;
    for scale in &coeffs.detail {
        for subband in scale {
            ac_energy += subband.iter().map(|c| c.norm_sqr()).sum::<f64>();
        }
    }
    ac_energy += coeffs.fine.iter().map(|c| c.norm_sqr()).sum::<f64>();

    let coarse_energy: f64 = coeffs.coarse.iter().map(|c| c.norm_sqr()).sum();
    let energy_ratio = if coarse_energy > 1e-12 {
        ac_energy / coarse_energy
    } else {
        ac_energy / (arr.len() as f64 + 1.0)
    };

    println!(
        r#"{{"ac_energy":{:.6},"coarse_energy":{:.6},"energy_ratio":{:.6},"rows":{},"cols":{},"scales":{}}}"#,
        ac_energy,
        coarse_energy,
        energy_ratio,
        arr.nrows(),
        arr.ncols(),
        args.scales,
    );
}
