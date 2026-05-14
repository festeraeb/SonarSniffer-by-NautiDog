use serde::Serialize;
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize)]
pub struct FloatHit {
    pub offset: usize,
    pub value: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct XorBlock {
    pub offset: usize,
    pub mask: u8,
    pub length: usize,
    pub ascii_preview: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct FirmwareLookupResult {
    pub file: String,
    pub bytes: usize,
    pub float_hits: Vec<FloatHit>,
    pub xor_blocks: Vec<XorBlock>,
    pub error_message: Option<String>,
}

const KNOWN_FLOATS: [f32; 14] = [
    8.43, 10.09, 11.79, 24.72, 14.10, 29.01, 40.05, 13.50, 13.30, 13.40, 4.20, 4.10, 6.80, 40.04,
];
const XOR_MASKS: [u8; 4] = [0x00, 0x45, 0xC5, 0x80];

pub fn analyze_firmware_file(path: &Path) -> FirmwareLookupResult {
    let bytes = match fs::read(path) {
        Ok(v) => v,
        Err(err) => {
            return FirmwareLookupResult {
                file: path.display().to_string(),
                bytes: 0,
                float_hits: Vec::new(),
                xor_blocks: Vec::new(),
                error_message: Some(format!("Failed to read firmware file: {err}")),
            }
        }
    };

    let float_hits = extract_float_identifiers(&bytes, &KNOWN_FLOATS);
    let xor_blocks = extract_xor_blocks(&bytes, &XOR_MASKS, 48);

    FirmwareLookupResult {
        file: path.display().to_string(),
        bytes: bytes.len(),
        float_hits,
        xor_blocks,
        error_message: None,
    }
}

fn extract_float_identifiers(data: &[u8], known_floats: &[f32]) -> Vec<FloatHit> {
    let mut found = Vec::new();
    let mut i = 0usize;
    while i + 4 <= data.len() {
        let value = f32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        for target in known_floats {
            if (value - *target).abs() < 0.0001 {
                found.push(FloatHit { offset: i, value });
                break;
            }
        }
        i += 1;
    }
    found
}

fn extract_xor_blocks(data: &[u8], masks: &[u8], min_block: usize) -> Vec<XorBlock> {
    let mut results = Vec::new();
    for mask in masks {
        let decoded = data.iter().map(|b| b ^ *mask).collect::<Vec<_>>();
        let mut current = Vec::new();
        let mut start = 0usize;

        for (i, b) in decoded.iter().enumerate() {
            if is_ascii_like(*b) {
                if current.is_empty() {
                    start = i;
                }
                current.push(*b);
            } else {
                if current.len() >= min_block {
                    results.push(XorBlock {
                        offset: start,
                        mask: *mask,
                        length: current.len(),
                        ascii_preview: sanitize_preview(&current),
                    });
                }
                current.clear();
            }
        }

        if current.len() >= min_block {
            results.push(XorBlock {
                offset: start,
                mask: *mask,
                length: current.len(),
                ascii_preview: sanitize_preview(&current),
            });
        }
    }

    results
}

fn is_ascii_like(b: u8) -> bool {
    (32..=126).contains(&b) || b == 9 || b == 10 || b == 13
}

fn sanitize_preview(bytes: &[u8]) -> String {
    let mut s = String::from_utf8_lossy(bytes).to_string();
    s = s.replace('\r', "\\r");
    s = s.replace('\n', "\\n");
    if s.len() > 160 {
        s.truncate(160);
    }
    s
}

// ── RSD file fingerprinting ───────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct RsdFingerprint {
    pub file: String,
    pub file_size: usize,
    pub record_count: usize,
    pub first_magic_bytes: Vec<u8>,
    pub error_message: Option<String>,
}

pub fn fingerprint_rsd(path: &Path, max_records: usize) -> RsdFingerprint {
    let data = match fs::read(path) {
        Ok(v) => v,
        Err(e) => {
            return RsdFingerprint {
                file: path.display().to_string(),
                file_size: 0,
                record_count: 0,
                first_magic_bytes: Vec::new(),
                error_message: Some(format!("read: {e}")),
            };
        }
    };
    let magic_len = 16.min(data.len());
    RsdFingerprint {
        file: path.display().to_string(),
        file_size: data.len(),
        record_count: max_records.min(data.len() / 64),
        first_magic_bytes: data[..magic_len].to_vec(),
        error_message: None,
    }
}
