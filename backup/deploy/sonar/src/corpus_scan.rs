use serde::Serialize;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const TARGET_EXTENSIONS: [&str; 7] = ["rsd", "sl1", "sl2", "sl3", "dat", "jsf", "xtf"];
const MAX_HITS: usize = 500;
const SKIP_DIR_NAMES: [&str; 8] = [
    ".git",
    "target",
    "node_modules",
    "venv",
    ".venv",
    "site-packages",
    "__pycache__",
    "dist",
];

#[derive(Debug, Clone, Serialize)]
pub struct CorpusFileHit {
    pub path: String,
    pub extension: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct CorpusScanResult {
    pub root: String,
    pub total_files_seen: usize,
    pub matched_files: usize,
    pub counts_by_extension: BTreeMap<String, usize>,
    pub hits: Vec<CorpusFileHit>,
    pub truncated: bool,
    pub error_message: Option<String>,
}

pub fn scan_corpus_dir(root: &Path) -> CorpusScanResult {
    let mut counts = BTreeMap::<String, usize>::new();
    for ext in TARGET_EXTENSIONS {
        counts.insert(ext.to_string(), 0);
    }

    let mut total_files_seen = 0usize;
    let mut hits = Vec::<CorpusFileHit>::new();
    let mut truncated = false;

    let walk_result = walk_dir(root, &mut |path| {
        total_files_seen += 1;

        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(|s| s.to_ascii_lowercase())
            .unwrap_or_default();

        if !TARGET_EXTENSIONS.contains(&ext.as_str()) {
            return;
        }

        if let Some(count) = counts.get_mut(&ext) {
            *count += 1;
        }

        if hits.len() < MAX_HITS {
            let bytes = fs::metadata(path).map(|m| m.len()).unwrap_or(0);
            hits.push(CorpusFileHit {
                path: path.display().to_string(),
                extension: ext,
                bytes,
            });
        } else {
            truncated = true;
        }
    });

    let matched_files = counts.values().sum();

    CorpusScanResult {
        root: root.display().to_string(),
        total_files_seen,
        matched_files,
        counts_by_extension: counts,
        hits,
        truncated,
        error_message: walk_result.err().map(|e| e.to_string()),
    }
}

fn walk_dir<F>(root: &Path, on_file: &mut F) -> Result<(), std::io::Error>
where
    F: FnMut(&Path),
{
    let mut stack = vec![PathBuf::from(root)];

    while let Some(dir) = stack.pop() {
        let rd = match fs::read_dir(&dir) {
            Ok(v) => v,
            Err(err) => {
                if dir == root {
                    return Err(err);
                }
                continue;
            }
        };

        for entry in rd {
            let entry = match entry {
                Ok(v) => v,
                Err(_) => continue,
            };
            let path = entry.path();
            let ft = match entry.file_type() {
                Ok(v) => v,
                Err(_) => continue,
            };

            if ft.is_dir() {
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .map(|s| s.to_ascii_lowercase())
                    .unwrap_or_default();
                if SKIP_DIR_NAMES.contains(&name.as_str()) {
                    continue;
                }
                stack.push(path);
            } else if ft.is_file() {
                on_file(&path);
            }
        }
    }

    Ok(())
}
