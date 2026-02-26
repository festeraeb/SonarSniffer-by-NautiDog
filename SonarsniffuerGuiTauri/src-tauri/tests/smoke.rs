use std::collections::HashSet;
use std::path::PathBuf;

use tauri_appsonarsniffer_lib::{outputs::PipelineOptions, run_pipeline_internal};

/// Smoke test: run the known-good fixture if present and validate core invariants.
/// Manual smoke: run with `cargo test --test smoke -- --ignored` once the fixture exists.
#[test]
#[ignore]
fn parse_fixture_smoke_if_present() {
    let fixture = PathBuf::from("../test files/25MAR25-0802-01.RSD");
    if !fixture.exists() {
        eprintln!("SKIP: fixture not present at {:?}", fixture);
        return;
    }

    let resp = run_pipeline_internal(
        fixture
            .to_str()
            .expect("fixture path should be valid UTF-8"),
        Some(PipelineOptions::default()),
        None,
    );

    assert!(
        resp.parse.error_message.is_none(),
        "parse error: {:?}",
        resp.parse.error_message
    );

    let ids: HashSet<u32> = resp.parse.channels.iter().map(|c| c.id).collect();
    for expected in [14u32, 15, 16, 17] {
        assert!(ids.contains(&expected), "missing channel id {expected}");
    }

    let depth_max = resp.depth_stats[1];
    assert!(depth_max > 0.0 && depth_max < 200.0, "depth_max out of range: {depth_max}");

    let temp_max = resp.temp_stats[1];
    assert!(temp_max >= 0.0 && temp_max < 60.0, "temp_max out of range: {temp_max}");

    assert!(resp.status.starts_with("Pipeline"), "status not OK: {}", resp.status);
}

/// Probe all .RSD files in the test-files directory and print the results.
/// Run with: cargo test --test smoke probe_all -- --nocapture
#[test]
fn probe_all_test_files() {
    use tauri_appsonarsniffer_lib::garmin_rsd_parser::GarminRSDParser;
    use std::path::Path;

    let test_dir = Path::new("../test files");
    if !test_dir.exists() {
        eprintln!("SKIP: test files dir not found");
        return;
    }

    let mut found = 0usize;
    let entries = std::fs::read_dir(test_dir).expect("read_dir");
    let mut paths: Vec<_> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("RSD"))
        .collect();
    paths.sort();

    let parser = GarminRSDParser::new();
    for path in &paths {
        found += 1;
        let probe = parser.probe_file(path);
        eprintln!("---");
        eprintln!("FILE : {}", path.file_name().unwrap().to_string_lossy());
        eprintln!("  size           : {} bytes", probe.file_size);
        eprintln!("  magic          : {:?} @ {:?}", probe.magic_found, probe.magic_offset);
        eprintln!("  header CRC     : {}", if probe.header_crc_ok { "OK" } else { "FAIL" });
        eprintln!("  body CRC       : {}", if probe.body_crc_ok   { "OK" } else { "FAIL" });
        eprintln!("  first channel  : {:?} ({})",
            probe.first_channel,
            probe.first_channel_label.as_deref().unwrap_or("?"));
        eprintln!("  body fields    : {:?}", probe.first_record_fields);
        eprintln!("  est. records   : {:?}", probe.estimated_records);
        eprintln!("  summary        : {}", probe.summary);
    }
    eprintln!("---");
    eprintln!("Probed {found} files.");
    assert!(found > 0, "no .RSD files found in {:?}", test_dir);
}
