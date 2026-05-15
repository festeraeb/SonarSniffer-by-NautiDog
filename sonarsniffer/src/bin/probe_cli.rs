/// Standalone CLI.
///
/// Modes (select with first arg after optional dir):
///   (default)  — probe summary for every supported sonar file in dir
///   meta       — walk & decode every varstruct in the metadata preamble of .RSD files
///   meta-one <file> — decode preamble of a single named file
///
/// Supported extensions: .rsd .sl2 .sl3 .dat .son .xtf .jsf .svlog
///
/// Usage examples:
///   cargo run --bin probe_cli
///   cargo run --bin probe_cli "../../test files" meta
///   cargo run --bin probe_cli "../../test files" meta-one "93SV-UHD-GT56.RSD"

fn le_u32(b: &[u8]) -> Option<u32> {
    if b.len() < 4 { return None; }
    Some(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
}
fn le_u16(b: &[u8]) -> Option<u16> {
    if b.len() < 2 { return None; }
    Some(u16::from_le_bytes([b[0], b[1]]))
}
fn le_u32_padded(b: &[u8]) -> Option<u32> {
    if b.is_empty() { return None; }
    let mut buf = [0u8; 4];
    let n = b.len().min(4);
    buf[..n].copy_from_slice(&b[..n]);
    Some(u32::from_le_bytes(buf))
}

fn read_varuint(bytes: &[u8], mut pos: usize, limit: usize) -> Result<(u32, usize), ()> {
    let mut result = 0u32;
    let mut shift = 0u32;
    while pos < limit && pos < bytes.len() {
        let b = bytes[pos]; pos += 1;
        result |= ((b & 0x7F) as u32) << shift;
        if b & 0x80 == 0 { return Ok((result, pos)); }
        shift += 7;
        if shift > 35 { return Err(()); }
    }
    Err(())
}

/// Parse a varstruct at `pos`, returning (fields, end_pos).
/// Returns Err if field_count > 50_000 or reading overflows.
fn parse_varstruct_raw(bytes: &[u8], mut pos: usize) -> Result<(Vec<(u32, Vec<u8>)>, usize), ()> {
    let limit = bytes.len();
    let (field_count, p) = read_varuint(bytes, pos, limit)?;
    pos = p;
    if field_count > 50_000 { return Err(()); }
    let mut fields = Vec::new();
    for _ in 0..field_count {
        let (key, p2) = read_varuint(bytes, pos, limit)?;
        pos = p2;
        let fn_id = key >> 3;
        let lc = key & 7;
        let vlen = if lc == 7 {
            let (v, p3) = read_varuint(bytes, pos, limit)?;
            pos = p3;
            v as usize
        } else {
            lc as usize
        };
        let end = pos.saturating_add(vlen);
        if end > bytes.len() { return Err(()); }
        fields.push((fn_id, bytes[pos..end].to_vec()));
        pos = end;
    }
    // skip 4-byte CRC
    if pos + 4 > bytes.len() { return Err(()); }
    pos += 4;
    Ok((fields, pos))
}

fn hex_str(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ")
}

fn maybe_string(b: &[u8]) -> Option<String> {
    if b.is_empty() { return None; }
    let s = std::str::from_utf8(b).ok()?;
    if s.chars().all(|c| c.is_ascii_graphic() || c == ' ') && s.len() > 1 {
        Some(s.to_string())
    } else {
        None
    }
}

fn decode_field_val(b: &[u8]) -> String {
    let hex = hex_str(b);
    let mut extras = vec![];
    if let Some(s) = maybe_string(b) { extras.push(format!("str=\"{s}\"")); }
    if let Some(v) = le_u32_padded(b) { extras.push(format!("u32={v}")); }
    if b.len() == 2 { if let Some(v) = le_u16(b) { extras.push(format!("u16={v}")); } }
    if extras.is_empty() { hex } else { format!("{hex}  ({})", extras.join(", ")) }
}

fn dump_preamble(path: &std::path::Path) {
    let bytes = std::fs::read(path).expect("read");
    // First record magic always at 20482 in all our test files.
    // Walk varstructs from 0 up to that limit.
    let first_magic_pos = {
        let magic_bytes = [0x86u8, 0xDA, 0xE9, 0xB7];
        let mut found = bytes.len();
        for i in 0..bytes.len().saturating_sub(4) {
            if bytes[i..i+4] == magic_bytes {
                found = i;
                break;
            }
        }
        found
    };

    println!("=== {} (first magic @ +{}) ===", path.file_name().unwrap().to_string_lossy(), first_magic_pos);

    let mut pos = 0usize;
    let mut vs_idx = 0usize;
    // Walk until we hit the first magic position
    while pos + 4 < first_magic_pos {
        match parse_varstruct_raw(&bytes[..first_magic_pos + 50], pos) {
            Ok((fields, end)) => {
                println!("  [VS#{vs_idx} @+{pos}..+{end}] {} fields", fields.len());
                for (fid, val) in &fields {
                    println!("    f{fid:02}: {}", decode_field_val(val));
                }
                vs_idx += 1;
                if end <= pos { break; }
                pos = end;
            }
            Err(_) => {
                // Try to skip a byte and re-sync
                pos += 1;
            }
        }
        if vs_idx > 500 { println!("  (stopping after 500 varstructs)"); break; }
    }
    println!();
}

fn main() {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let dir = if !args.is_empty() && !args[0].starts_with("meta") {
        args.remove(0)
    } else {
        "../../test files".to_string()
    };
    let mode = args.get(0).map(String::as_str).unwrap_or("probe").to_string();
    let single = args.get(1).cloned();

    let test_dir = std::path::Path::new(&dir);
    if !test_dir.exists() {
        eprintln!("Directory not found: {}", test_dir.display());
        std::process::exit(1);
    }

    let mut paths: Vec<_> = if test_dir.is_file() {
        vec![test_dir.to_path_buf()]
    } else {
        std::fs::read_dir(test_dir)
            .expect("read_dir")
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .and_then(|x| x.to_str())
                    .map(|s| {
                        matches!(
                            s.to_ascii_lowercase().as_str(),
                            "rsd" | "sl2" | "sl3" | "dat" | "son" | "xtf" | "jsf" | "svlog" | "bin"
                        )
                    })
                    .unwrap_or(false)
            })
            .collect()
    };
    paths.sort();

    match mode.as_str() {
        "meta" => {
            for path in &paths { dump_preamble(path); }
        }
        "meta-one" => {
            let name = single.expect("meta-one requires a filename argument");
            let path = paths.iter().find(|p| {
                p.file_name().unwrap().to_string_lossy().eq_ignore_ascii_case(&name)
            }).unwrap_or_else(|| { eprintln!("File not found: {name}"); std::process::exit(1); });
            dump_preamble(path);
        }
        "heuristic" => {
            use sonarsniffer_lib::probing::probe_file_bytes;

            println!("\n╔══════════════════════════════════════════════════════════════╗");
            println!("║          HeuristicProbe — Hardware Fingerprint Report         ║");
            println!("╚══════════════════════════════════════════════════════════════╝");

            for path in &paths {
                let fname = path.file_name().unwrap().to_string_lossy();
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(e) => { println!("\n[SKIP] {fname}: {e}"); continue; }
                };

                let report = probe_file_bytes(&data);

                println!("\n┌─────────────────────────────────────────────────────────────");
                println!("│ FILE     : {fname}");
                println!("│ SIZE     : {:.2} MB  |  probed: {:.2} MB  |  records: {}",
                    data.len() as f64 / 1_048_576.0,
                    report.bytes_read as f64 / 1_048_576.0,
                    report.records_decoded);
                println!("│ HARDWARE : {}  (confidence {:.0}%)",
                    report.hardware, (report.confidence * 100.0) as u32);
                println!("│ GENERATION: {:?}", report.generation);
                println!("├─────────────── channels ───────────────────────────────────");

                for ch in &report.channels {
                    let flip_icon = match ch.flip_status {
                        sonarsniffer_lib::probing::FlipStatus::Flipped       => " ⚠ FLIP",
                        sonarsniffer_lib::probing::FlipStatus::Normal        => "",
                        sonarsniffer_lib::probing::FlipStatus::Indeterminate => " ?flip",
                    };
                    println!("│  ch{:<3}  role={:<24}  nadir={:<8}  gap={:<5}  bit={:?}  noise={:.0}  records={}{}",
                        ch.channel_id,
                        format!("{:?}", ch.suggested_role),
                        format!("{:?}", ch.nadir_edge),
                        ch.nadir_gap_samples,
                        ch.bit_depth,
                        ch.noise_floor,
                        ch.records_seen,
                        flip_icon,
                    );
                }

                // Summarise what the Mosaic Engine will do with this file
                println!("├─────────────── mosaic interpretation ──────────────────────");
                let has_single_port = report.channels.iter()
                    .any(|c| matches!(c.suggested_role, sonarsniffer_lib::probing::SuggestedRole::SingleSidePort));
                let has_single_star = report.channels.iter()
                    .any(|c| matches!(c.suggested_role, sonarsniffer_lib::probing::SuggestedRole::SingleSideStarboard));
                let has_paired = report.channels.iter()
                    .any(|c| matches!(c.suggested_role,
                        sonarsniffer_lib::probing::SuggestedRole::PairedPort |
                        sonarsniffer_lib::probing::SuggestedRole::PairedStarboard));
                let flip_count = report.channels.iter()
                    .filter(|c| matches!(c.flip_status, sonarsniffer_lib::probing::FlipStatus::Flipped))
                    .count();

                if has_single_port || has_single_star {
                    println!("│  GT51 MODE: use each wing as a full independent swath");
                    println!("│    Port  wing → water-column at sample[0], NOT split");
                    println!("│    Star  wing → water-column at sample[max], NOT split");
                }
                if has_paired { println!("│  PAIRED MODE: port+starboard nadir-seam stitch"); }
                if flip_count > 0 {
                    println!("│  ⚠  {} channel(s) require samples.reverse() before render", flip_count);
                }
                println!("└─────────────────────────────────────────────────────────────");
            }
            println!("\nDone — {} files probed.", paths.len());
        }
        "process" => {
            // Full pipeline: parse → build all outputs
            use sonarsniffer_lib::outputs::{build_outputs, PipelineOptions};
            use sonarsniffer_lib::garmin_rsd_parser::GarminRSDParser;

            for path in &paths {
                let fname = path.file_name().unwrap().to_string_lossy();
                println!("\n═══ Processing: {} ═══", fname);

                let mut parser = GarminRSDParser::new();
                let parsed = parser.parse_file(path);
                println!("  Parsed: {} records, {} pings", parsed.record_count, parsed.pings.len());

                if parsed.pings.is_empty() {
                    println!("  SKIP: no pings parsed");
                    continue;
                }

                let mut opts = PipelineOptions::default();
                // Use output dir next to the file
                opts.output_dir = Some(path.parent().unwrap_or(std::path::Path::new("."))
                    .join("output").to_string_lossy().to_string());

                match build_outputs(path, &parsed, &opts, None, None) {
                    Ok(summary) => {
                        println!("  Output dir: {}", summary.output_dir);
                        for art in &summary.artifacts {
                            println!("    [{}] {} — {}", art.kind, art.path.split('/').last().unwrap_or(&art.path), art.details);
                        }
                    }
                    Err(e) => {
                        println!("  ERROR: {:#}", e);
                    }
                }
            }
        }
        _ => {
            for path in &paths {
                let detected = sonarsniffer_lib::format_detector::detect_and_parse(path);
                let probe = &detected.probe;
                let parse = &detected.parse;
                // For Garmin files the preamble channels are populated; for others it will be empty.
                let ch_labels: Vec<String> = probe.preamble_channels.iter().map(|&ch| {
                    let label = sonarsniffer_lib::garmin_rsd_parser::map_channel_info(ch)
                        .map(|(scan, family)| format!("{scan}/{family}"))
                        .unwrap_or_else(|| "?".to_string());
                    format!("{ch} ({label})")
                }).collect();
                let channel_summary: Vec<String> = parse.channels.iter()
                    .map(|c| format!("{} [{}]", c.name, parse.channel_counts.get(&c.id).copied().unwrap_or(0)))
                    .collect();
                println!("---");
                println!("FILE   : {}", path.file_name().unwrap().to_string_lossy());
                println!("FORMAT : {}", detected.format);
                println!("  size           : {} bytes", probe.file_size);
                if !probe.preamble_channels.is_empty() {
                    println!("  preamble chans : [{}]", ch_labels.join(", "));
                }
                if !channel_summary.is_empty() {
                    println!("  channels       : {}", channel_summary.join(", "));
                }
                println!("  records        : {}", parse.record_count);
                if let Some(est) = probe.estimated_records {
                    println!("  est. records   : {est}");
                }
                if let Some(err) = &parse.error_message {
                    println!("  ERROR          : {err}");
                }
                println!("  summary        : {}", probe.summary);
            }
            println!("---");
            println!("Probed {} files.", paths.len());
        }
    }
}
