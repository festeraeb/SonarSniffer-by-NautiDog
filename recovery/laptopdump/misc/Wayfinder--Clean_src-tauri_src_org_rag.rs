// RAG-friendly system state extraction for org/classification operations
// These are helper functions that format data for RAG prompts

use std::collections::{HashMap, HashSet};
use std::path::Path;

pub fn format_ext_summary(by_ext: &HashMap<String, usize>) -> String {
    let mut lines = String::new();
    for (ext, count) in by_ext.iter() {
        lines.push_str(&format!("- .{}: {} files\n", ext, count));
    }
    lines
}

pub fn format_classifications(items: &[serde_json::Value]) -> String {
    let mut lines = String::new();
    for item in items.iter().take(3) {
        if let (Some(path), Some(proj), Some(conf)) = (
            item["path"].as_str(),
            item["project"].as_str(),
            item["confidence"].as_f64(),
        ) {
            lines.push_str(&format!(
                "- `{}`\n  → Project: {}\n  → Confidence: {:.0}%\n\n",
                path,
                proj,
                conf * 100.0
            ));
        }
    }
    lines
}

pub fn format_moves(items: &[serde_json::Value]) -> String {
    let mut lines = String::new();
    for item in items.iter().take(5) {
        if let (Some(from), Some(to), Some(when)) = (
            item["from"].as_str(),
            item["to"].as_str(),
            item["when"].as_str(),
        ) {
            lines.push_str(&format!(
                "- {} → {}\n  ({})\n",
                from, to, when
            ));
        }
    }
    lines
}

pub fn format_structure(folders: &HashMap<String, HashSet<String>>) -> String {
    let mut lines = String::new();
    for (folder, _files) in folders.iter().take(5) {
        lines.push_str(&format!("- {}/\n", folder));
    }
    lines
}

pub fn format_unclassified(items: &[serde_json::Value]) -> String {
    let mut lines = String::new();
    for item in items {
        if let (Some(path), Some(name)) = (item["path"].as_str(), item["name"].as_str()) {
            lines.push_str(&format!("- `{}`\n  filename: {}\n", path, name));
        }
    }
    lines
}

pub fn build_rag_context_document(
    total_files: usize,
    by_ext: &HashMap<String, usize>,
    current_classifications: &[serde_json::Value],
    recent_moves: &[serde_json::Value],
    folder_structure: &HashMap<String, HashSet<String>>,
    unclassified: &[serde_json::Value],
) -> String {
    format!(
        r#"
# WAYFINDER ORG SYSTEM STATE

**Generated**: {}
**Total Files in Index**: {}

## FILE TYPE SUMMARY
{}

## ACTIVE CLASSIFICATIONS (Examples)
{}

## RECENT MOVES (What we did before)
{}

## FOLDER STRUCTURE
{}

## UNCLASSIFIED FILES (Need sorting)
{}

---

## YOUR RULES
When classifying files, look for these keywords:
- projectA: sonar, sniffer, cesarops, Drift, garmin, drone, rsd
- projectB: bag, bagfile, pdf, redaction, masking, magnetic

Min confidence: 0.8
Ambiguity delta: 0.3 (if top 2 scores too close, mark as unsorted)

---

## EXAMPLES OF CORRECT CLASSIFICATION

File: "sonar_processing_2025.py"
- Keywords found: "sonar"
- Match: projectA (sonar is in projectA keywords)
- Confidence: 0.95
- Action: Move to projectA folder

File: "magnetic_anomaly_detection.rs"
- Keywords found: "magnetic"
- Match: projectB (magnetic is in projectB keywords)
- Confidence: 0.92
- Action: Move to projectB folder

File: "readme_notes.txt"
- Keywords found: none
- Confidence: 0.1 (below 0.8 minimum)
- Action: Mark as unsorted (needs human review)

---
"#,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
        total_files,
        format_ext_summary(&by_ext),
        format_classifications(&current_classifications),
        format_moves(&recent_moves),
        format_structure(&folder_structure),
        format_unclassified(&unclassified),
    )
}
