/// Multi-format sonar file dispatcher.
///
/// `detect_and_parse()` is the single public entry point.  It inspects the
/// file extension and (as a fallback) the first few magic bytes, then delegates
/// to the appropriate parser.  All parsers return `garmin_rsd_parser::ParseResult`
/// so the pipeline, outputs, and frontend remain format-agnostic.
///
/// Supported formats
/// -----------------
///   .rsd / .RSD            Garmin Sonar Log (the original format)
///   .sl2                   Lowrance SL2 sidescan
///   .sl3                   Lowrance SL3 sidescan
///   .dat  (.SON siblings)  Humminbird multi-file recording
///   .son                   Humminbird SON beam file (finds sibling DAT automatically)
///   .xtf                   Triton/Exail XTF (eXtended Triton Format)
///   .jsf                   EdgeTech JSF (Jacobs Sonar Format)
///   .svlog                 Blue Robotics / Cerulean Ping Protocol log
///   .bin                   Blue Robotics raw serial dump (content-sniffed as Ping Protocol)
///   (unknown extension)    Content sniff against all known magic bytes
use crate::garmin_rsd_parser::{FileProbe, GarminRSDParser, ParseResult};
use std::path::Path;

/// Format tag returned alongside `ParseResult` so callers can log/display it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SonarFormat {
    GarminRSD,
    LowranceSL2,
    LowranceSL3,
    Humminbird,
    XTF,
    JSF,
    Cerulean,
    Unknown,
}

impl std::fmt::Display for SonarFormat {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::GarminRSD => "Garmin RSD",
            Self::LowranceSL2 => "Lowrance SL2",
            Self::LowranceSL3 => "Lowrance SL3",
            Self::Humminbird => "Humminbird DAT/SON",
            Self::XTF => "Triton XTF",
            Self::JSF => "EdgeTech JSF",
            Self::Cerulean => "Blue Robotics / Cerulean",
            Self::Unknown => "Unknown",
        };
        write!(f, "{s}")
    }
}

/// Result of the format-detection + parsing stage.
pub struct DetectResult {
    pub format: SonarFormat,
    /// Standard parse result (channels, pings, errors) from the matching parser.
    pub parse: ParseResult,
    /// Garmin-specific pre-parse probe.  Present (with real data) only for Garmin
    /// RSD files; for all other formats a minimal placeholder is returned.
    pub probe: FileProbe,
}

/// Detect the sonar format and parse the file.  This is the main entry point
/// used by `lib.rs` instead of calling `GarminRSDParser` directly.
pub fn detect_and_parse(path: &Path) -> DetectResult {
    let format = detect_format(path);

    match format {
        SonarFormat::GarminRSD => {
            let mut parser = GarminRSDParser::new();
            let probe = parser.probe_file(path);
            let parse = parser.parse_file(path);
            DetectResult {
                format,
                parse,
                probe,
            }
        }

        SonarFormat::LowranceSL2 | SonarFormat::LowranceSL3 => {
            let parse = crate::lowrance_parser::parse_file(path);
            DetectResult {
                format,
                parse,
                probe: placeholder_probe(path),
            }
        }

        SonarFormat::Humminbird => {
            let parse = crate::humminbird_parser::parse_file(path);
            DetectResult {
                format,
                parse,
                probe: placeholder_probe(path),
            }
        }

        SonarFormat::XTF => {
            let parse = crate::xtf_parser::parse_file(path);
            DetectResult {
                format,
                parse,
                probe: placeholder_probe(path),
            }
        }

        SonarFormat::JSF => {
            let parse = crate::jsf_parser::parse_file(path);
            DetectResult {
                format,
                parse,
                probe: placeholder_probe(path),
            }
        }

        SonarFormat::Cerulean => {
            let parse = crate::cerulean_parser::parse_file(path);
            DetectResult {
                format,
                parse,
                probe: placeholder_probe(path),
            }
        }

        SonarFormat::Unknown => {
            // Fall back to Garmin probe so we at least get a diagnostic error message
            let parser = GarminRSDParser::new();
            let probe = parser.probe_file(path);
            let parse = ParseResult::unknown_format(path);
            DetectResult {
                format,
                parse,
                probe,
            }
        }
    }
}

// ── format detection ──────────────────────────────────────────────────────────

fn detect_format(path: &Path) -> SonarFormat {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    // Primary: extension-based dispatch
    match ext.as_str() {
        "rsd" => return SonarFormat::GarminRSD,
        "sl2" => return SonarFormat::LowranceSL2,
        "sl3" => return SonarFormat::LowranceSL3,
        "dat" => return SonarFormat::Humminbird,
        "son" => return SonarFormat::Humminbird,
        "xtf" => return SonarFormat::XTF,
        "jsf" => return SonarFormat::JSF,
        "svlog" => return SonarFormat::Cerulean,
        _ => {}
    }

    // Secondary: read first 32 bytes and sniff magic
    let header = match read_header(path, 32) {
        Some(h) => h,
        None => return SonarFormat::Unknown,
    };

    sniff_magic(&header)
}

fn sniff_magic(header: &[u8]) -> SonarFormat {
    if header.len() < 4 {
        return SonarFormat::Unknown;
    }

    // Garmin RSD magic  (LE u32 0xB7E9DA86)
    if header[0] == 0x86 && header[1] == 0xDA && header[2] == 0xE9 && header[3] == 0xB7 {
        return SonarFormat::GarminRSD;
    }
    // Garmin RSD magic variants (0xB7E9DA87 / 88 / 89)
    if header[0] == 0x87 && header[1] == 0xDA && header[2] == 0xE9 && header[3] == 0xB7 {
        return SonarFormat::GarminRSD;
    }

    // Humminbird SON magic  (BE u32 0xC0DEAB21)
    if header[0] == 0xC0 && header[1] == 0xDE && header[2] == 0xAB && header[3] == 0x21 {
        return SonarFormat::Humminbird;
    }

    // Lowrance SL2 magic  (LE u16 format=2, version u16)
    if header.len() >= 8 {
        let fmt = u16::from_le_bytes([header[0], header[1]]);
        if fmt == 2 {
            return SonarFormat::LowranceSL2;
        }
        if fmt == 3 {
            return SonarFormat::LowranceSL3;
        }
    }

    // XTF packet magic 0xFACE (LE u16) — check both at offset 0 and offset 1024 is
    // not feasible with only 32 bytes; XTF file header magic may be at byte 0
    if header.len() >= 2 {
        let m = u16::from_le_bytes([header[0], header[1]]);
        if m == 0xFACE {
            return SonarFormat::XTF;
        }
    }

    // JSF start marker 0x1601
    if header.len() >= 2 {
        let m = u16::from_le_bytes([header[0], header[1]]);
        if m == 0x1601 {
            return SonarFormat::JSF;
        }
    }

    // Blue Robotics Ping Protocol 'B','R'
    if header[0] == b'B' && header[1] == b'R' {
        return SonarFormat::Cerulean;
    }

    SonarFormat::Unknown
}

fn read_header(path: &Path, n: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path).ok()?;
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf).ok()?;
    buf.truncate(read);
    Some(buf)
}

// ── placeholder probe for non-Garmin formats ──────────────────────────────────

fn placeholder_probe(path: &Path) -> FileProbe {
    let file_size = std::fs::metadata(path)
        .map(|m| m.len() as usize)
        .unwrap_or(0);
    FileProbe {
        file_size,
        magic_found: None,
        magic_offset: None,
        header_crc_ok: false,
        body_crc_ok: false,
        first_channel: None,
        first_channel_label: None,
        first_record_fields: Vec::new(),
        estimated_records: None,
        preamble_channels: Vec::new(),
        summary: format!("{} bytes", file_size),
    }
}
