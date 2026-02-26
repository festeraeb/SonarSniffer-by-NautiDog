use std::collections::HashMap;
use std::f32;

/// Scan for float-based unit identifiers in firmware (e.g., 8.43, 10.09, ...)
fn extract_float_identifiers(data: &[u8], known_floats: &[f32]) -> Vec<(usize, f32)> {
    let mut found = Vec::new();
    let mut i = 0;
    while i + 4 <= data.len() {
        let val = f32::from_le_bytes([data[i], data[i+1], data[i+2], data[i+3]]);
        for &target in known_floats {
            if (val - target).abs() < 0.0001 {
                found.push((i, val));
            }
        }
        i += 1;
    }
    found
}

/// XOR-decode a region with a given mask
fn xor_decode_region(data: &[u8], mask: u8) -> Vec<u8> {
    data.iter().map(|b| b ^ mask).collect()
}

/// Scan for XOR-masked blocks (using known masks)
fn extract_xor_blocks(data: &[u8], masks: &[u8], min_block: usize) -> Vec<(usize, u8, Vec<u8>)> {
    let mut results = Vec::new();
    for &mask in masks {
        let decoded = xor_decode_region(data, mask);
        // Heuristic: look for long ASCII runs in decoded data
        let mut buf = Vec::new();
        let mut start = 0;
        for (i, &b) in decoded.iter().enumerate() {
            if (b >= 32 && b <= 126) || b == 9 || b == 10 || b == 13 {
                buf.push(b);
            } else {
                if buf.len() >= min_block {
                    results.push((start, mask, buf.clone()));
                }
                buf.clear();
                start = i + 1;
            }
        }
        if buf.len() >= min_block {
            results.push((start, mask, buf));
        }
    }
    results
}
// Rust CLI for fast ASCII/UTF-16LE string and magic-byte extraction
// Usage: cargo run --release -- <input_file> <output_dir> [min_ascii] [min_utf16]

use std::fs::{File, create_dir_all};
use std::io::{BufReader, Read, Write};
use std::path::Path;

pub fn extract_ascii_strings(data: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    for &b in data {
        if (b >= 32 && b <= 126) || b == 9 || b == 10 || b == 13 {
            buf.push(b);
        } else {
            if buf.len() >= min_len {
                if let Ok(s) = String::from_utf8(buf.clone()) {
                    out.push(s);
                }
            }
            buf.clear();
        }
    }
    if buf.len() >= min_len {
        if let Ok(s) = String::from_utf8(buf) {
            out.push(s);
        }
    }
    return out;
}

pub fn extract_utf16le_strings(data: &[u8], min_len: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut buf = Vec::new();
    let mut i = 0;
    while i + 1 < data.len() {
        let lo = data[i];
        let hi = data[i + 1];
        if hi == 0 && (lo >= 32 && lo <= 126) {
            buf.push(lo);
            buf.push(hi);
        } else {
            if buf.len() >= min_len * 2 {
                if let Ok(s) = String::from_utf16(
                    &buf.chunks(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>(),
                ) {
                    out.push(s);
                }
            }
            buf.clear();
        }
        i += 2;
    }
    if buf.len() >= min_len * 2 {
        if let Ok(s) = String::from_utf16(
            &buf.chunks(2).map(|b| u16::from_le_bytes([b[0], b[1]])).collect::<Vec<_>>(),
        ) {
            out.push(s);
        }
    }
    out
}


pub fn run(input: &str, outdir: &str, min_ascii: usize, min_utf16: usize) {
    if let Err(e) = create_dir_all(outdir) {
        eprintln!("Failed to create output directory {}: {}", outdir, e);
        std::process::exit(1);
    }
    let data = match File::open(input) {
        Ok(f) => {
            let mut f = BufReader::new(f);
            let mut buf = Vec::new();
            if let Err(e) = f.read_to_end(&mut buf) {
                eprintln!("Failed to read input file {}: {}", input, e);
                std::process::exit(1);
            }
            buf
        },
        Err(e) => {
            eprintln!("Failed to open input file {}: {}", input, e);
            std::process::exit(1);
        }
    };
    let ascii = extract_ascii_strings(&data, min_ascii);
    let utf16 = extract_utf16le_strings(&data, min_utf16);
    let ascii_path = Path::new(outdir).join("ascii.txt");
    let utf16_path = Path::new(outdir).join("utf16.txt");
    let mut fa = match File::create(&ascii_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create ascii.txt: {}", e);
            std::process::exit(1);
        }
    };
    let mut fu = match File::create(&utf16_path) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("Failed to create utf16.txt: {}", e);
            std::process::exit(1);
        }
    };

    // --- Firmware float identifier extraction ---
    let known_floats = [8.43, 10.09, 11.79, 24.72, 14.10, 29.01, 40.05, 13.50, 13.30, 13.40, 4.20, 4.10, 6.80, 40.04];
    let float_hits = extract_float_identifiers(&data, &known_floats);
    let float_json_path = Path::new(outdir).join("float_identifiers.json");
    let mut float_map = HashMap::new();
    for (ofs, val) in &float_hits {
        float_map.insert(format!("0x{:X}", ofs), val);
    }
    let _ = std::fs::write(&float_json_path, serde_json::to_string_pretty(&float_map).unwrap());

    // --- XOR-masked block extraction ---
    let xor_masks = [0x00, 0x45, 0xC5, 0x80];
    let xor_blocks = extract_xor_blocks(&data, &xor_masks, 32);
    let xor_json_path = Path::new(outdir).join("xor_blocks.json");
    let mut xor_vec = Vec::new();
    for (ofs, mask, block) in xor_blocks {
        if let Ok(s) = String::from_utf8(block.clone()) {
            xor_vec.push(serde_json::json!({
                "offset": format!("0x{:X}", ofs),
                "mask": format!("0x{:02X}", mask),
                "ascii": s
            }));
        }
    }
    let _ = std::fs::write(&xor_json_path, serde_json::to_string_pretty(&xor_vec).unwrap());
    for s in ascii {
        if let Err(e) = writeln!(fa, "{}", s.replace("\r", "\\r").replace("\n", "\\n")) {
            eprintln!("Failed to write to ascii.txt: {}", e);
            std::process::exit(1);
        }
    }
    for s in utf16 {
        if let Err(e) = writeln!(fu, "{}", s.replace("\r", "\\r").replace("\n", "\\n")) {
            eprintln!("Failed to write to utf16.txt: {}", e);
            std::process::exit(1);
        }
    }
    match fa.metadata() {
        Ok(meta) => print!("ASCII output size: {} bytes. ", meta.len()),
        Err(_) => print!("ASCII output size: unknown. ")
    }
    match fu.metadata() {
        Ok(meta) => println!("UTF16 output size: {} bytes.", meta.len()),
        Err(_) => println!("UTF16 output size: unknown.")
    }
    println!("Float identifiers written to {:?}", float_json_path);
    println!("XOR-masked blocks written to {:?}", xor_json_path);
    println!("Extraction complete.");
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn test_ascii() {
        let data = b"hello\nworld\x00\x01test";
        let out = extract_ascii_strings(data, 3);
        assert!(out.contains(&"hello".to_string()));
        assert!(out.contains(&"world".to_string()));
    }
    #[test]
    fn test_utf16() {
        let data = b"h\x00e\x00l\x00l\x00o\x00\x00\x00t\x00e\x00s\x00t\x00";
        let out = extract_utf16le_strings(data, 3);
        assert!(out.contains(&"hello".to_string()));
        assert!(out.contains(&"test".to_string()));
    }
}
