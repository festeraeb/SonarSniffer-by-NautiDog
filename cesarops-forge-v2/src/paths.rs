//! Canonical filesystem paths for forge (override via env on each node).

pub const DEFAULT_PROJECT_ROOT: &str = "/codebase/repos/wreckhunter2000-1";
pub const DEFAULT_FORGE_V2_DIR: &str = "/codebase/repos/wreckhunter2000-1/cesarops-forge-v2";

pub fn project_root() -> String {
    std::env::var("CESAROPS_PROJECT_ROOT").unwrap_or_else(|_| DEFAULT_PROJECT_ROOT.to_string())
}

pub fn forge_v2_dir() -> String {
    std::env::var("CESAROPS_FORGE_V2_DIR").unwrap_or_else(|_| DEFAULT_FORGE_V2_DIR.to_string())
}

pub fn aeromagnetic_worker_binary() -> String {
    let root = project_root();
    let candidates = [
        format!("{}/target/release/cesarops-aeromagnetic-worker", root),
        "/codebase/repos/wreckhunter2000-1/target/release/cesarops-aeromagnetic-worker".to_string(),
        "/home/cesarops/wreckhunter2000-1/target/release/cesarops-aeromagnetic-worker".to_string(),
    ];
    let fallback = candidates[0].clone();
    candidates
        .into_iter()
        .find(|p| std::path::Path::new(p).exists())
        .unwrap_or(fallback)
}
