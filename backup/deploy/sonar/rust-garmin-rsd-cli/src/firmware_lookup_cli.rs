// CLI tool to extract float-based unit identifiers and XOR-masked blocks from Garmin firmware
// Usage: cargo run --bin firmware_lookup_cli <firmware_path>

use std::env;
use std::fs::File;
use std::io::Read;
mod firmware_lookup;
use firmware_lookup::extract_lookup_tables;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <firmware_path>", args[0]);
        return;
    }
    let firmware_path = &args[1];
    let mut file = match File::open(firmware_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to open firmware file: {}", e);
            return;
        }
    };
    let mut data = Vec::new();
    if let Err(e) = file.read_to_end(&mut data) {
        eprintln!("Failed to read firmware file: {}", e);
        return;
    }
    let (float_hits, xor_blocks) = extract_lookup_tables(&data);
    println!("Float-based unit identifiers:");
    for (ofs, val) in float_hits {
        println!("  Offset 0x{:X}: {:.2}", ofs, val);
    }
    println!("\nXOR-masked block candidates:");
    for (start, mask, block) in xor_blocks {
        let block: Vec<u8> = block;
        println!("  Start 0x{:X}, Mask 0x{:02X}, Length {}", start, mask, block.len());
    }
}
