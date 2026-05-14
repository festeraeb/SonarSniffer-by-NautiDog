// xor_blocks_sampler.rs
// Tool: Extracts a sample of XOR-masked blocks from a large xor_blocks.json file for quick inspection.
// Usage: cargo run --bin xor_blocks_sampler -- <input_json> <output_json> <sample_count>

use std::env;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use serde_json::{Value, from_reader, to_writer_pretty};

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 4 {
        eprintln!("Usage: {} <input_json> <output_json> <sample_count>", args[0]);
        std::process::exit(1);
    }
    let input_path = &args[1];
    let output_path = &args[2];
    let sample_count: usize = args[3].parse().unwrap_or(10);

    let file = File::open(input_path).expect("Failed to open input JSON");
    let reader = BufReader::new(file);
    let blocks: Value = from_reader(reader).expect("Failed to parse JSON");
    let arr = blocks.as_array().expect("Expected JSON array");

    let mut sample = Vec::new();
    for (i, block) in arr.iter().enumerate() {
        if i >= sample_count { break; }
        sample.push(block);
    }

    let out_file = File::create(output_path).expect("Failed to create output JSON");
    let writer = BufWriter::new(out_file);
    to_writer_pretty(writer, &sample).expect("Failed to write output JSON");
    println!("Wrote {} sample blocks to {}", sample.len(), output_path);
}
