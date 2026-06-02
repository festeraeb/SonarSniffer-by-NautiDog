//! Curvelet band-pass filter: suppress low-frequency (broad shoal/terrain) and
//! high-frequency (noise), keep detail scales (hull-sized features).
//!
//! Input/output: raw f32 with 8-byte header (rows_u32_le, cols_u32_le).
//! Default: zero coarse + fine, keep ALL detail scales → isolates mid-frequency
//! directional structure (wrecks, ridges, hulls) against flat background.

use clap::Parser;
use ndarray::Array2;
use nauticuvs_full::{curvelet_forward, curvelet_inverse};
use num_complex::Complex;
use std::fs::File;
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "curvelet_bandpass")]
struct Args {
    /// Input raw f32 file (8-byte header: rows_u32_le cols_u32_le, then row-major f32)
    #[arg(long)]
    input: PathBuf,
    /// Output raw f32 file (same format)
    #[arg(long)]
    output: PathBuf,
    /// Number of curvelet scales
    #[arg(long, default_value_t = 5)]
    scales: usize,
    /// Keep the coarse (DC/low-freq) band?
    #[arg(long)]
    keep_coarse: bool,
    /// Keep the fine (highest-freq) band?
    #[arg(long)]
    keep_fine: bool,
    /// Detail scales to keep (0-indexed, comma-separated). Default: all.
    #[arg(long)]
    keep_scales: Option<String>,
}

fn main() {
    let args = Args::parse();
    let (arr, rows, cols) = read_raw_f32(&args.input);
    eprintln!("Input: {rows}x{cols}, scales={}", args.scales);

    let mut coeffs = curvelet_forward(&arr, args.scales).unwrap_or_else(|e| {
        eprintln!("curvelet_forward: {e}");
        std::process::exit(1);
    });

    if !args.keep_coarse {
        coeffs.coarse.fill(Complex::new(0.0, 0.0));
        eprintln!("  zeroed coarse band");
    }
    if !args.keep_fine {
        coeffs.fine.fill(Complex::new(0.0, 0.0));
        eprintln!("  zeroed fine band");
    }
    if let Some(keep_str) = &args.keep_scales {
        let keep: Vec<usize> = keep_str.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        for (s, scale_bands) in coeffs.detail.iter_mut().enumerate() {
            if !keep.contains(&s) {
                for band in scale_bands.iter_mut() {
                    band.fill(Complex::new(0.0, 0.0));
                }
                eprintln!("  zeroed detail scale {s}");
            }
        }
        eprintln!("  kept detail scales: {keep:?}");
    } else {
        eprintln!("  kept ALL {} detail scales", coeffs.detail.len());
    }

    let filtered = curvelet_inverse(&coeffs).unwrap_or_else(|e| {
        eprintln!("curvelet_inverse: {e}");
        std::process::exit(1);
    });

    write_raw_f32(&args.output, &filtered);

    let mean = filtered.iter().sum::<f32>() / filtered.len() as f32;
    let std_val = (filtered.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / filtered.len() as f32).sqrt();
    let peak = filtered.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
    eprintln!("Output: {}x{}, std={std_val:.4}, peak={peak:.4}", filtered.nrows(), filtered.ncols());
}

fn read_raw_f32(path: &PathBuf) -> (Array2<f32>, usize, usize) {
    let mut f = File::open(path).unwrap_or_else(|e| { eprintln!("open {}: {e}", path.display()); std::process::exit(1); });
    let mut hdr = [0u8; 8];
    f.read_exact(&mut hdr).unwrap();
    let rows = u32::from_le_bytes(hdr[0..4].try_into().unwrap()) as usize;
    let cols = u32::from_le_bytes(hdr[4..8].try_into().unwrap()) as usize;
    let mut buf = vec![0u8; rows * cols * 4];
    f.read_exact(&mut buf).unwrap();
    let data: Vec<f32> = buf.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
    (Array2::from_shape_vec((rows, cols), data).unwrap(), rows, cols)
}

fn write_raw_f32(path: &PathBuf, arr: &Array2<f32>) {
    let mut f = File::create(path).unwrap_or_else(|e| { eprintln!("create {}: {e}", path.display()); std::process::exit(1); });
    let (rows, cols) = (arr.nrows(), arr.ncols());
    f.write_all(&(rows as u32).to_le_bytes()).unwrap();
    f.write_all(&(cols as u32).to_le_bytes()).unwrap();
    for &v in arr.iter() {
        f.write_all(&v.to_le_bytes()).unwrap();
    }
}
