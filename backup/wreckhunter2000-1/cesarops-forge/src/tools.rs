use anyhow::{Context, Result};
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::Command;
use tracing::{error, info};

#[derive(Debug)]
pub struct CompilerError {
    pub message: String,
    pub level: String,
}

impl fmt::Display for CompilerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "[{}] {}", self.level, self.message)
    }
}

/// Runs cargo check and returns any compiler errors found.
/// Empty vec = success.
pub fn cargo_check(project_dir: &Path) -> Result<Vec<CompilerError>> {
    info!("Running cargo check in {:?}", project_dir);

    let output = Command::new("cargo")
        .arg("check")
        .arg("--message-format=json")
        .current_dir(project_dir)
        .output()
        .context("Failed to execute cargo check")?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut errors = Vec::new();

    for line in stdout.lines() {
        let msg: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // cargo --message-format=json nests: {"reason":"compiler-message","message":{"message":"...","level":"error"}}
        if msg.get("reason").and_then(|r| r.as_str()) != Some("compiler-message") {
            continue;
        }

        if let Some(compiler_msg) = msg.get("message") {
            let text = compiler_msg.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error")
                .to_string();
            let level = compiler_msg.get("level")
                .and_then(|l| l.as_str())
                .unwrap_or("unknown")
                .to_string();

            if level == "error" {
                errors.push(CompilerError { message: text, level });
            }
        }
    }

    let error_count = errors.len();
    if error_count > 0 {
        error!("cargo check found {} errors", error_count);
    } else {
        info!("cargo check passed");
    }

    Ok(errors)
}

/// Writes files to a directory, creating parent dirs as needed.
pub fn write_files(dir: &Path, files: &[(String, String)]) -> Result<()> {
    for (path, content) in files {
        let full_path = dir.join(path);
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut file = fs::File::create(&full_path)
            .with_context(|| format!("Failed to create {}", full_path.display()))?;
        file.write_all(content.as_bytes())?;
        info!("Wrote: {}", path);
    }
    Ok(())
}

/// Recursively copies a directory tree from src to dest.
pub fn copy_dir_recursive(src: &Path, dest: &Path) -> Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        // Skip target/ and .git/ directories
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == "target" || name_str == ".git" {
            continue;
        }

        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            fs::copy(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Commits files from temp dir to target dir (recursive, skips target/.git).
pub fn commit_files(temp_dir: &Path, target_dir: &Path) -> Result<()> {
    info!("Committing files from {:?} to {:?}", temp_dir, target_dir);
    copy_dir_recursive(temp_dir, target_dir)?;
    info!("Files committed successfully");
    Ok(())
}
