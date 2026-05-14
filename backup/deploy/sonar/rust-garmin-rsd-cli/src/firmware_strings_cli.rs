// Standalone CLI for firmware string extraction
// Usage: cargo run --bin firmware_strings_cli -- <input_file> <output_dir> [min_ascii] [min_utf16]

pub mod firmware_strings;
use std::env;
// use std::path::Path;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        eprintln!("Usage: {} <input_file> <output_dir> [min_ascii] [min_utf16]", args[0]);
        std::process::exit(1);
    }
    let input = &args[1];
    let outdir = &args[2];
    let min_ascii = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(5);
    let min_utf16 = args.get(4).and_then(|s| s.parse().ok()).unwrap_or(4);
    firmware_strings::run(input, outdir, min_ascii, min_utf16);
}
