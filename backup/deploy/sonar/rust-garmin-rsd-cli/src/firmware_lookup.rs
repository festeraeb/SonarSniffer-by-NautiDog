//! Garmin Firmware Lookup Table Extraction Library
//
// This Rust module provides reusable functions for extracting float-based unit identifiers
// and XOR-masked configuration blocks from Garmin firmware blobs. It is designed for backend
// integration and can be called from CLI, Tauri, or Python FFI.

pub fn extract_float_identifiers(data: &[u8], known_floats: &[f32]) -> Vec<(usize, f32)> {
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
pub fn xor_decode_region(data: &[u8], mask: u8) -> Vec<u8> {
    data.iter().map(|b| b ^ mask).collect()
}

/// Scan for XOR-masked blocks (using known masks)
pub fn extract_xor_blocks(data: &[u8], masks: &[u8], min_block: usize) -> Vec<(usize, u8, Vec<u8>)> {
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

/// Example: Extract all lookup table candidates from firmware
pub fn extract_lookup_tables(data: &[u8]) -> (Vec<(usize, f32)>, Vec<(usize, u8, Vec<u8>)>) {
    let known_floats = [8.43, 10.09, 11.79, 24.72, 14.10, 29.01, 40.05, 13.50, 13.30, 13.40, 4.20, 4.10, 6.80, 40.04];
    let float_hits = extract_float_identifiers(data, &known_floats);
    let xor_masks = [0x00, 0x45, 0xC5, 0x80];
    let xor_blocks = extract_xor_blocks(data, &xor_masks, 32);
    (float_hits, xor_blocks)
}
