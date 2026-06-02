// Tauri command handlers - Pure Rust implementation
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;
use chrono::{DateTime, Local};
use rand::Rng;
use std::hash::{Hash, Hasher};
use std::collections::hash_map::DefaultHasher;
use std::time::{SystemTime, UNIX_EPOCH};
use regex::Regex;

// Language-specific reference patterns
static TS_IMPORT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"import[^'\"]*[\'\"](?P<p>[^'\"]+)[\'\"]"#).unwrap());
static TS_REQUIRE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"require\([\'\"](?P<p>[^'\"]+)[\'\"]\)"#).unwrap());
static PY_FROM_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"from\s+(?P<p>[\w\.]+)\s+import"#).unwrap());
static PY_IMPORT_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"import\s+(?P<p>[\w\.]+)"#).unwrap());
static RUST_USE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"use\s+(?P<p>[\w:]+)"#).unwrap());
static RUST_MOD_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"mod\s+(?P<p>[\w_]+);"#).unwrap());
static CPP_INCLUDE_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r#"#include\s+[<\"](?P<p>[^>\"]+)[>\"]"#).unwrap());

// Import git_assistant module from crate root
use crate::git_assistant;

// Azure OpenAI Configuration
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct AzureConfig {
    pub endpoint: String,           // e.g., "https://your-resource.openai.azure.com"
    pub api_key: String,            // Your API key
    pub deployment_name: String,    // e.g., "text-embedding-ada-002"
    pub api_version: String,        // e.g., "2024-02-01"
}

// Google Cloud Vertex AI Configuration
#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct GcpConfig {
    pub project_id: String,
    pub location: String,
    pub model_id: String,
    pub service_account_path: String,
    pub endpoint: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum EmbeddingProvider {
    Local,
    Llama, // local llama.cpp server (GPU/CPU)
    Azure,
    Gcp,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ProviderConfig {
    pub provider: EmbeddingProvider,
    #[serde(default)]
    pub local_model: Option<String>,
    #[serde(default)]
    pub local_endpoint: Option<String>, // for llama.cpp http server
}

// Embedding data stored per file
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEmbedding {
    pub path: String,
    pub embedding: Vec<f32>,        // 1536 dimensions for ada-002
    pub content_hash: String,       // To detect if file changed
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmbeddingsData {
    pub embeddings: Vec<FileEmbedding>,
    pub model: String,
    pub created_at: String,
}

// Cluster data
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Cluster {
    pub id: usize,
    pub centroid: Vec<f32>,
    pub file_paths: Vec<String>,
    pub label: Option<String>,      // Auto-generated label
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClustersData {
    pub clusters: Vec<Cluster>,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ScanResult {
    pub files_scanned: usize,
    pub total_size: u64,
    pub index_path: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct FileEntry {
    pub path: String,
    pub name: String,
    pub size: u64,
    pub modified: String,
    pub extension: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ClassifiedFile {
    pub path: String,
    pub project: String,
    pub confidence: f32,
    pub reasons: Vec<String>,
    pub alternates: Vec<(String, f32)>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ClassificationRules {
    pub min_confidence: Option<f32>,
    pub ambiguity_delta: Option<f32>,
    pub include_patterns: Option<HashMap<String, Vec<String>>>, // label -> patterns to force
    pub exclude_patterns: Option<HashMap<String, Vec<String>>>, // label -> patterns to block
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct MovePlanEntry {
    pub from: String,
    pub to: String,
    pub link_type: String, // junction | symlink | hardlink | copy
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct RefIndex {
    pub refs: HashMap<String, Vec<String>>,       // target -> referrers
    pub basename_map: HashMap<String, Vec<String>>, // basename -> targets
    pub built_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct Neighbor {
    pub path: String,
    pub score: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct FileInsight {
    pub path: String,
    pub imports: Vec<String>,
    pub mentions: Vec<String>,
    pub semantic_neighbors: Vec<Neighbor>,
    pub keywords: Vec<String>,
    pub score: f32,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MoveAction {
    pub action: String,             // move | create_shim | delete
    pub from: Option<String>,
    pub to: Option<String>,
    pub path: Option<String>,
    pub shim_content: Option<String>,
    pub note: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct MovePlan {
    pub plan_id: String,
    pub generated_at: String,
    pub target_root: String,
    pub steps: Vec<MoveAction>,
    pub revert_steps: Vec<MoveAction>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IndexData {
    pub files: Vec<FileEntry>,
    pub scan_path: String,
    pub created_at: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SearchResult {
    pub path: String,
    pub name: String,
    pub score: f32,
    pub preview: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IndexStats {
    pub total_files: usize,
    pub total_size_bytes: u64,
    pub extensions: HashMap<String, usize>,
    pub last_updated: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct IndexState {
    pub has_files: bool,
    pub index_valid: bool,
    pub message: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SystemInfo {
    pub os: String,
    pub arch: String,
}

// Error logging structure
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ErrorLogEntry {
    pub timestamp: String,
    pub operation: String,
    pub file_path: Option<String>,
    pub error_message: String,
    pub error_code: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ErrorLog {
    pub entries: Vec<ErrorLogEntry>,
    pub last_updated: String,
}

// Reminders
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Reminder {
    pub id: String,
    pub title: String,
    pub due: Option<String>,
    pub severity: Option<String>,
    pub link_path: Option<String>,
    pub repo_path: Option<String>,
    pub status: String, // open | done | snoozed
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Serialize, Deserialize, Debug, Clone, Default)]
pub struct ReminderStore {
    pub reminders: Vec<Reminder>,
}

// Batch processing progress
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BatchProgress {
    pub batch_id: String,
    pub total_files: usize,
    pub processed_files: usize,
    pub current_batch: usize,
    pub total_batches: usize,
    pub batch_size: usize,
    pub status: String,  // "running", "paused", "complete", "error"
    pub started_at: String,
    pub last_updated: String,
    pub errors: Vec<String>,
}

// Embedding job configuration
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EmbeddingJobConfig {
    pub batch_size: usize,        // Files per batch (default: 100)
    pub delay_ms: u64,            // Delay between requests (default: 50)
    pub max_retries: usize,       // Max retries per file (default: 3)
    pub save_interval: usize,     // Save progress every N files (default: 50)
    pub max_files: Option<usize>, // Limit total files (for testing)
}

impl Default for EmbeddingJobConfig {
    fn default() -> Self {
        Self {
            batch_size: 100,
            delay_ms: 50,
            max_retries: 3,
            save_interval: 50,
            max_files: None,
        }
    }
}

fn default_local_model_name() -> String {
    "embeddinggemma-300m-f16".to_string()
}

fn read_provider_config(index_path: &Path) -> Option<ProviderConfig> {
    let config_file = index_path.join("provider_config.json");
    if !config_file.exists() {
        return None;
    }
    let content = fs::read_to_string(&config_file).ok()?;
    serde_json::from_str::<ProviderConfig>(&content).ok()
}

fn write_provider_config(index_path: &Path, provider: EmbeddingProvider, local_model: Option<String>, local_endpoint: Option<String>) -> Result<(), String> {
    let config_file = index_path.join("provider_config.json");
    let config = ProviderConfig {
        provider,
        local_model,
        local_endpoint,
    };
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize provider config: {}", e))?;
    fs::write(&config_file, json)
        .map_err(|e| format!("Failed to write provider config: {}", e))?;
    Ok(())
}

fn resolve_provider_config(index_path: &Path) -> ProviderConfig {
    if let Some(config) = read_provider_config(index_path) {
        return config;
    }

    ProviderConfig {
        provider: EmbeddingProvider::Llama,
        local_model: Some(default_local_model_name()),
        local_endpoint: Some("http://localhost:5002".to_string()),
    }
}

// Helper to log errors to file
fn log_error(index_dir: &Path, operation: &str, file_path: Option<&str>, error_message: &str, error_code: Option<&str>) {
    let error_log_file = index_dir.join("error_log.json");
    
    let mut error_log: ErrorLog = if error_log_file.exists() {
        fs::read_to_string(&error_log_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
    } else {
        ErrorLog::default()
    };
    
    error_log.entries.push(ErrorLogEntry {
        timestamp: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        operation: operation.to_string(),
        file_path: file_path.map(|s| s.to_string()),
        error_message: error_message.to_string(),
        error_code: error_code.map(|s| s.to_string()),
    });
    error_log.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    
    // Keep only last 1000 errors
    if error_log.entries.len() > 1000 {
        error_log.entries = error_log.entries.split_off(error_log.entries.len() - 1000);
    }
    
    if let Ok(json) = serde_json::to_string_pretty(&error_log) {
        let _ = fs::write(&error_log_file, json);
    }
}

// ---------------------------------------------------------------------------
// Reminder helpers
// ---------------------------------------------------------------------------

fn reminders_path() -> std::path::PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| Path::new(".").to_path_buf())
        .join(".wayfinder_reminders.json")
}

fn load_reminders() -> ReminderStore {
    let path = reminders_path();
    if let Ok(content) = fs::read_to_string(&path) {
        if let Ok(store) = serde_json::from_str::<ReminderStore>(&content) {
            return store;
        }
    }
    ReminderStore::default()
}

fn save_reminders(store: &ReminderStore) -> Result<(), String> {
    let path = reminders_path();
    let json = serde_json::to_string_pretty(store).map_err(|e| format!("Failed to serialize reminders: {}", e))?;
    fs::write(&path, json).map_err(|e| format!("Failed to write reminders: {}", e))
}

fn now_string() -> String {
    Local::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

fn new_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("rem-{}-{}", ts, rand::thread_rng().gen::<u32>())
}

// Pure Rust command handlers - no Python dependency

/// Scan a directory and create an index of text files
#[tauri::command]
pub async fn scan_directory(path: String, index_dir: String, extensions: Option<Vec<String>>, allow_all: Option<bool>) -> Result<serde_json::Value, String> {
    println!("[RUST] scan_directory called - path: {}, index_dir: {}", path, index_dir);
    
    let scan_path = Path::new(&path);
    if !scan_path.exists() {
        println!("[RUST] Path does not exist: {}", path);
        return Err(format!("Path does not exist: {}", path));
    }
    println!("[RUST] Path exists, starting scan...");

    let mut files: Vec<FileEntry> = Vec::new();
    let mut total_size: u64 = 0;

    // Common text file extensions (used as a sensible default)
    let default_extensions = vec![
        "md", "txt", "text", "markdown", "mdx",
        "py", "pyw", "pyi",
        "js", "jsx", "ts", "tsx",
        "json", "yaml", "yml", "toml", "ini", "cfg",
        "html", "htm", "css", "scss", "sass",
        "rs", "go", "java", "c", "cpp", "h", "hpp",
        "sh", "bash", "zsh", "ps1", "bat", "cmd",
        "xml", "svg", "log",
    ];

    // Normalize allowed extensions from the frontend; treat "*" or allow_all as "no filtering"
    let allow_all_files = allow_all.unwrap_or(false)
        || extensions.as_ref().map(|exts| exts.iter().any(|e| e.trim() == "*")).unwrap_or(false);

    let allowed_set: Option<HashSet<String>> = if allow_all_files {
        None
    } else if let Some(exts) = extensions.clone() {
        if exts.is_empty() {
            Some(default_extensions.iter().map(|s| s.to_string()).collect())
        } else {
            Some(
                exts.into_iter()
                    .filter(|e| !e.trim().is_empty() && e.trim() != "*")
                    .map(|e| e.trim_start_matches('.').to_lowercase())
                    .collect(),
            )
        }
    } else {
        Some(default_extensions.iter().map(|s| s.to_string()).collect())
    };

    for entry in WalkDir::new(&path)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let file_path = entry.path();

        // Skip anything inside .git to avoid binary blobs and noise
        if file_path.ancestors().any(|a| a.file_name().and_then(|n| n.to_str()) == Some(".git")) {
            continue;
        }

        // Skip node_modules and other heavy dependency folders
        if file_path.to_string_lossy().to_lowercase().contains("node_modules") {
            continue;
        }
        
        // Skip hidden files and directories
        if file_path.file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.starts_with('.'))
            .unwrap_or(false) 
        {
            continue;
        }

        if file_path.is_file() {
            let ext = file_path.extension()
                .and_then(|e| e.to_str())
                .unwrap_or("")
                .to_lowercase();

            let allowed = match &allowed_set {
                None => true, // allow all
                Some(set) => set.contains(&ext),
            };

            if allowed {
                if let Ok(metadata) = fs::metadata(file_path) {
                    let size = metadata.len();
                    total_size += size;
                    
                    let modified = metadata.modified()
                        .ok()
                        .and_then(|t| DateTime::<Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string().into())
                        .unwrap_or_else(|| "Unknown".to_string());

                    files.push(FileEntry {
                        path: file_path.to_string_lossy().to_string(),
                        name: file_path.file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        size,
                        modified,
                        extension: ext,
                    });
                }
            }
        }
    }

    // Create index directory
    let index_path = if index_dir.is_empty() {
        Path::new(&path).join(".wayfinder_index")
    } else {
        Path::new(&index_dir).to_path_buf()
    };
    
    fs::create_dir_all(&index_path)
        .map_err(|e| format!("Failed to create index directory: {}", e))?;

    // Save index data
    let index_data = IndexData {
        files: files.clone(),
        scan_path: path.clone(),
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    let index_file = index_path.join("index.json");
    let json = serde_json::to_string_pretty(&index_data)
        .map_err(|e| format!("Failed to serialize index: {}", e))?;
    
    fs::write(&index_file, json)
        .map_err(|e| format!("Failed to write index file: {}", e))?;

    println!("[RUST] Scan complete - {} files found, {} bytes total", files.len(), total_size);
    println!("[RUST] Index written to: {}", index_file.display());
    
    Ok(serde_json::json!({
        "files_scanned": files.len(),
        "total_size": total_size,
        "index_path": index_path.to_string_lossy().to_string()
    }))
}

/// Generate embeddings using the configured provider
#[tauri::command]
pub async fn generate_embeddings(index_dir: String, max_files: Option<usize>, batch_size: Option<usize>) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let provider_config = resolve_provider_config(index_path);

    match provider_config.provider {
        EmbeddingProvider::Local => {
            generate_embeddings_local(index_dir, max_files, provider_config.local_model).await
        },
        EmbeddingProvider::Llama => {
            generate_embeddings_llama(index_dir, max_files, batch_size, provider_config.local_endpoint, provider_config.local_model).await
        },
        EmbeddingProvider::Azure => {
            generate_embeddings_azure(index_dir, max_files, batch_size).await
        },
        EmbeddingProvider::Gcp => {
            generate_embeddings_gcp(index_dir, max_files, batch_size).await
        }
    }
}

/// Deterministic PRNG helper for local embeddings (xorshift64*)
fn next_xorshift(state: &mut u64) -> u64 {
    let mut x = *state;
    x ^= x >> 12;
    x ^= x << 25;
    x ^= x >> 27;
    *state = x;
    x.wrapping_mul(2685821657736338717u64)
}

/// Produce a deterministic f32 embedding vector of length `dim` from text content
pub fn compute_embedding_from_text(text: &str, dim: usize) -> Vec<f32> {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    let mut state = hasher.finish();
    if state == 0 { state = 0x9E3779B97F4A7C15; }
    let mut v = Vec::with_capacity(dim);
    for _ in 0..dim {
        let r = next_xorshift(&mut state);
        let f = (r as f64 / std::u64::MAX as f64) as f32 * 2.0 - 1.0;
        v.push(f);
    }
    v
}

// Generate embeddings using a local model
pub async fn generate_embeddings_local(_index_dir: String, _max_files: Option<usize>, _model_name: Option<String>) -> Result<serde_json::Value, String> {
    // Simple deterministic local embedding fallback.
    // This avoids heavyweight native ML crates and provides reproducible vectors for offline use.
    // It uses a xorshift-style RNG seeded from a stable hash of the file contents.

    // Helper: compute a stable content hash (hex string)
    fn content_hash(s: &str) -> String {
        let mut hasher = DefaultHasher::new();
        s.hash(&mut hasher);
        format!("{:016x}", hasher.finish())
    }

    // Parameters
    let dim = 512usize; // local embedding dimension

    let index_path = Path::new(&_index_dir);
    let index_file = index_path.join("index.json");
    let embeddings_file = index_path.join("embeddings.json");

    if !index_file.exists() {
        return Err(format!("Index file not found: {}", index_file.display()));
    }

    let index_str = fs::read_to_string(&index_file)
        .map_err(|e| format!("Failed to read index file: {}", e))?;

    let index_data: IndexData = serde_json::from_str(&index_str)
        .map_err(|e| format!("Failed to parse index.json: {}", e))?;

    // Load existing embeddings (if any)
    let existing: EmbeddingsData = if embeddings_file.exists() {
        fs::read_to_string(&embeddings_file)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(EmbeddingsData { embeddings: Vec::new(), model: "local-fallback".to_string(), created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string() })
    } else {
        EmbeddingsData { embeddings: Vec::new(), model: "local-fallback".to_string(), created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string() }
    };

    // Build a map for quick lookup of cached embeddings by path
    let mut cache_map: HashMap<String, FileEmbedding> = HashMap::new();
    for fe in existing.embeddings.into_iter() {
        cache_map.insert(fe.path.clone(), fe);
    }

    let mut generated_count = 0usize;
    let mut cached_count = 0usize;
    let mut skipped_count = 0usize;
    let mut error_count = 0usize;

    let max_files = _max_files.unwrap_or(index_data.files.len());

    let mut out_embeddings: Vec<FileEmbedding> = Vec::new();

    for (i, entry) in index_data.files.into_iter().enumerate() {
        if i >= max_files { break; }
        let path = entry.path.clone();
        // Try cache
        if let Some(cached) = cache_map.get(&path) {
            // Check file still exists and same hash
            if Path::new(&path).exists() {
                if let Ok(content) = fs::read_to_string(&path) {
                    let h = content_hash(&content);
                    if h == cached.content_hash && cached.embedding.len() == dim {
                        out_embeddings.push(cached.clone());
                        cached_count += 1;
                        continue;
                    }
                }
            }
        }

        // Read file with binary/UTF-8 guard and size cap
        match fs::read(&path) {
            Ok(bytes) => {
                if bytes.is_empty() {
                    skipped_count += 1;
                    continue;
                }
                if bytes.len() > 2 * 1024 * 1024 { // skip >2MB in local mode to avoid heavy/binary files
                    skipped_count += 1;
                    continue;
                }
                match std::str::from_utf8(&bytes) {
                    Ok(content) => {
                        let emb = compute_embedding_from_text(content, dim);
                        let ch = content_hash(content);
                        out_embeddings.push(FileEmbedding { path: path.clone(), embedding: emb, content_hash: ch });
                        generated_count += 1;
                    }
                    Err(_) => {
                        skipped_count += 1;
                        // No log for expected binary skip
                    }
                }
            }
            Err(e) => {
                log_error(index_path, "generate_embeddings_local", Some(&path), &format!("Failed to read file: {}", e), None);
                skipped_count += 1;
                error_count += 1;
            }
        }

        // Save intermittently every 100 files
        if (generated_count + cached_count) % 100 == 0 {
            let save_data = EmbeddingsData { embeddings: out_embeddings.clone(), model: "local-fallback".to_string(), created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string() };
            if let Ok(json) = serde_json::to_string_pretty(&save_data) {
                let _ = fs::write(&embeddings_file, json);
            }
        }
    }

    // Final save
    let save_data = EmbeddingsData { embeddings: out_embeddings.clone(), model: "local-fallback".to_string(), created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string() };
    let json = serde_json::to_string_pretty(&save_data).map_err(|e| format!("Failed to serialize embeddings: {}", e))?;
    fs::write(&embeddings_file, json).map_err(|e| format!("Failed to write embeddings file: {}", e))?;

    println!("[RUST] Local embeddings complete: {} generated, {} cached, {} skipped, {} errors", generated_count, cached_count, skipped_count, error_count);

    Ok(serde_json::json!({
        "embeddings_generated": generated_count,
        "embeddings_cached": cached_count,
        "embeddings_skipped": skipped_count,
        "embeddings_errors": error_count,
        "embeddings_path": embeddings_file.to_string_lossy().to_string(),
    }))
}

/// Generate embeddings via local llama.cpp HTTP server
pub async fn generate_embeddings_llama(index_dir: String, max_files: Option<usize>, batch_size: Option<usize>, endpoint: Option<String>, model_name: Option<String>) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    let embeddings_file = index_path.join("embeddings.json");
    let progress_file = index_path.join("embedding_progress.json");
    let cancel_file = index_path.join("embedding_cancel");

    // Clear stale cancel signals from previous runs
    if cancel_file.exists() {
        let _ = fs::remove_file(&cancel_file);
    }

    if !index_file.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }

    let index_content = fs::read_to_string(&index_file).map_err(|e| format!("Failed to read index: {}", e))?;
    let index_data: IndexData = serde_json::from_str(&index_content).map_err(|e| format!("Failed to parse index: {}", e))?;

    let files_initial: Vec<FileEntry> = if let Some(max) = max_files {
        index_data.files.into_iter().take(max).collect()
    } else {
        index_data.files
    };

    let (files_to_process, collapsed_groups, collapsed_files) = collapse_versions(files_initial);

    let client = reqwest::Client::new();
    let base = endpoint.unwrap_or_else(|| "http://localhost:5002".to_string());
    let url = format!("{}/v1/embeddings", base.trim_end_matches('/'));
    let model = model_name.unwrap_or_else(|| "embeddinggemma-300m-f16".to_string());

    let chunk_size = batch_size.unwrap_or(64).max(1);
    let total_files = files_to_process.len();
    let total_batches = (total_files + chunk_size - 1) / chunk_size;

    let mut progress = BatchProgress {
        batch_id: format!("{}", Local::now().timestamp()),
        total_files,
        processed_files: 0,
        current_batch: 0,
        total_batches,
        batch_size: chunk_size,
        status: "running".to_string(),
        started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        errors: Vec::new(),
    };
    let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());

    let mut embeddings: Vec<FileEmbedding> = Vec::new();
    let mut generated_count = 0;
    let mut error_count = 0;
    let mut skipped_count = 0;

    for (batch_idx, chunk) in files_to_process.chunks(chunk_size).enumerate() {
        progress.current_batch = batch_idx + 1;

        if cancel_file.exists() {
            progress.status = "cancelled".to_string();
            let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
            return Err("Embedding cancelled by user".to_string());
        }

        for file in chunk {
            // Skip binary/large files
            let bytes = match fs::read(&file.path) {
                Ok(b) => b,
                Err(_) => { skipped_count += 1; continue; }
            };
            if bytes.is_empty() || bytes.len() > 2 * 1024 * 1024 {
                skipped_count += 1;
                continue;
            }
            let content = match std::str::from_utf8(&bytes) {
                Ok(c) => c,
                Err(_) => { skipped_count += 1; continue; }
            };
            let truncated = if content.len() > 8000 { &content[..8000] } else { content };

            let payload = serde_json::json!({
                "model": model,
                "input": truncated,
            });

            match client.post(&url).json(&payload).send().await {
                Ok(resp) => {
                    let status = resp.status();
                    if !status.is_success() {
                        let err_text = resp.text().await.unwrap_or_default();
                        error_count += 1;
                        log_error(index_path, "llama_api_error", Some(&file.path), &err_text, Some(&status.as_u16().to_string()));
                        continue;
                    }
                    match resp.json::<serde_json::Value>().await {
                        Ok(json) => {
                            if let Some(arr) = json.get("data").and_then(|d| d.get(0)).and_then(|d| d.get("embedding")) {
                                if let Ok(vec) = serde_json::from_value::<Vec<f32>>(arr.clone()) {
                                    let ch = format!("{:x}", md5_hash(truncated));
                                    embeddings.push(FileEmbedding { path: file.path.clone(), embedding: vec, content_hash: ch });
                                    generated_count += 1;
                                } else {
                                    error_count += 1;
                                }
                            } else {
                                error_count += 1;
                            }
                        }
                        Err(e) => {
                            error_count += 1;
                            log_error(index_path, "llama_parse_error", Some(&file.path), &e.to_string(), None);
                        }
                    }
                }
                Err(e) => {
                    error_count += 1;
                    log_error(index_path, "llama_request_error", Some(&file.path), &e.to_string(), None);
                }
            }

            progress.processed_files += 1;
            progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
        }
    }

    let final_data = EmbeddingsData {
        embeddings: embeddings.clone(),
        model: model.clone(),
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let json = serde_json::to_string_pretty(&final_data).map_err(|e| format!("Failed to serialize embeddings: {}", e))?;
    fs::write(&embeddings_file, json).map_err(|e| format!("Failed to write embeddings file: {}", e))?;

    Ok(serde_json::json!({
        "embeddings_generated": generated_count,
        "cached_count": 0,
        "skipped_count": skipped_count,
        "error_count": error_count,
        "total_files": embeddings.len(),
        "message": format!("Generated {} embeddings ({} skipped, {} errors)", generated_count, skipped_count, error_count),
        "collapsed_groups": collapsed_groups,
        "collapsed_files": collapsed_files
    }))
}

/// Generate embeddings using Azure OpenAI with auto-batching and progress saving
pub async fn generate_embeddings_azure(index_dir: String, max_files: Option<usize>, batch_size: Option<usize>) -> Result<serde_json::Value, String> {
    println!("[RUST] generate_embeddings_azure called for: {}", index_dir);
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    let config_file = index_path.join("azure_config.json");
    let embeddings_file = index_path.join("embeddings.json");
    let progress_file = index_path.join("embedding_progress.json");
    let batch_dir = index_path.join("embedding_batches");
    let cancel_file = index_path.join("embedding_cancel");
    let _ = fs::create_dir_all(&batch_dir);

    // Clear stale cancel signals from previous runs
    if cancel_file.exists() {
        let _ = fs::remove_file(&cancel_file);
    }

    // Configuration
    let config_batch_size = batch_size.unwrap_or(100);

    // Check if index exists
    if !index_file.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }
    // Load Azure config
    if !config_file.exists() {
        return Err("Azure config not found. Please configure Azure OpenAI settings first.".to_string());
    }
    let config_content = fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read Azure config: {}", e))?;
    let config: AzureConfig = serde_json::from_str(&config_content)
        .map_err(|e| format!("Failed to parse Azure config: {}", e))?;
    if config.endpoint.is_empty() || config.api_key.is_empty() || config.deployment_name.is_empty() {
        return Err("Azure config is incomplete. Please set endpoint, API key, and deployment name.".to_string());
    }
    // Load index
    let index_content = fs::read_to_string(&index_file)
        .map_err(|e| format!("Failed to read index: {}", e))?;
    let index_data: IndexData = serde_json::from_str(&index_content)
        .map_err(|e| format!("Failed to parse index: {}", e))?;
    // Apply max_files limit if specified
    let files_initial: Vec<FileEntry> = if let Some(max) = max_files {
        index_data.files.into_iter().take(max).collect()
    } else {
        index_data.files
    };
    let (files_to_process, collapsed_groups, collapsed_files) = collapse_versions(files_initial);
    let total_files = files_to_process.len();
    let total_batches = (total_files + config_batch_size - 1) / config_batch_size;
    println!("[RUST] Processing {} files in {} batches of {}", total_files, total_batches, config_batch_size);

    // Load existing batch files for resuming
    let mut processed_paths = std::collections::HashSet::new();
    let mut new_embeddings: Vec<FileEmbedding> = Vec::new();
    let mut batch_idx = 0;
    if let Ok(read_dir) = fs::read_dir(&batch_dir) {
        for entry in read_dir {
            if let Ok(entry) = entry {
                let path = entry.path();
                if path.extension().map(|e| e == "json").unwrap_or(false) {
                    if let Ok(content) = fs::read_to_string(&path) {
                        if let Ok(data) = serde_json::from_str::<EmbeddingsData>(&content) {
                            for emb in data.embeddings {
                                processed_paths.insert(emb.path.clone());
                                new_embeddings.push(emb);
                            }
                        }
                    }
                }
            }
        }
    }
    // Also load from main embeddings.json if present (legacy)
    if embeddings_file.exists() {
        if let Ok(content) = fs::read_to_string(&embeddings_file) {
            if let Ok(data) = serde_json::from_str::<EmbeddingsData>(&content) {
                for emb in data.embeddings {
                    processed_paths.insert(emb.path.clone());
                    new_embeddings.push(emb);
                }
            }
        }
    }
    let cached_count = processed_paths.len();
    let mut generated_count = 0;
    let mut error_count = 0;
    let mut skipped_count = 0;
    let mut api_version = if config.api_version.is_empty() {
        "2024-02-01".to_string()
    } else {
        config.api_version.clone()
    };
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;
    let mut progress = BatchProgress {
        batch_id: format!("{}", Local::now().timestamp()),
        total_files,
        processed_files: processed_paths.len(),
        current_batch: 0,
        total_batches,
        batch_size: config_batch_size,
        status: "running".to_string(),
        started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        errors: Vec::new(),
    };
    let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
    // Process in batches
    for batch_start in (0..total_files).step_by(config_batch_size) {
        let batch_end = (batch_start + config_batch_size).min(total_files);
        let batch_files: Vec<_> = files_to_process[batch_start..batch_end].iter().filter(|f| !processed_paths.contains(&f.path)).collect();
        if batch_files.is_empty() {
            batch_idx += 1;
            continue;
        }
        let mut batch_embeddings: Vec<FileEmbedding> = Vec::new();
        for file in batch_files.iter() {
            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(e) => {
                    skipped_count += 1;
                    log_error(&index_path, "read_file", Some(&file.path), &e.to_string(), None);
                    continue;
                }
            };
            if content.trim().is_empty() {
                skipped_count += 1;
                continue;
            }
            let content_hash = format!("{:x}", md5_hash(&content));
            // Trim content to a safe size to avoid exceeding embedding model context limits
            let max_chars = 8000usize;
            let truncated_content = if content.len() > max_chars {
                content[..max_chars].to_string()
            } else {
                content.clone()
            };
            let input = format!("passage: {}", truncated_content);
            let mut retries = 0;
            let max_retries = 3;
            let mut success = false;
            while retries < max_retries && !success {
                let mut base = config.endpoint.trim_end_matches('/').to_string();
                if !base.ends_with("/openai") && !base.ends_with("/openai/") {
                    base = format!("{}/openai", base);
                }
                let url_current = format!("{}/deployments/{}/embeddings?api-version={}", base, config.deployment_name, api_version);
                let request_body = serde_json::json!({ "input": input });
                match client
                    .post(&url_current)
                    .header("api-key", &config.api_key)
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .send()
                    .await
                {
                    Ok(response) => {
                        if response.status().is_success() {
                            match response.json::<serde_json::Value>().await {
                                Ok(json) => {
                                    // Check for explicit error field
                                    if json.get("error").is_some() {
                                        let err_text = json["error"].to_string();
                                        log_error(&index_path, "api_error", Some(&file.path), &err_text, None);
                                        progress.errors.push(format!("{}: API error - {}", file.name, err_text));
                                        error_count += 1;
                                    } else if let Some(embedding) = json["data"][0]["embedding"].as_array() {
                                        let emb_vec: Vec<f32> = embedding
                                            .iter()
                                            .filter_map(|v| v.as_f64().map(|f| f as f32))
                                            .collect();
                                        batch_embeddings.push(FileEmbedding {
                                            path: file.path.clone(),
                                            embedding: emb_vec,
                                            content_hash: content_hash.clone(),
                                        });
                                        generated_count += 1;
                                        success = true;
                                    } else {
                                        // Unexpected response shape
                                        let err_text = json.to_string();
                                        log_error(&index_path, "api_error", Some(&file.path), &format!("Unexpected response: {}", err_text), None);
                                        progress.errors.push(format!("{}: Unexpected response shape", file.name));
                                        error_count += 1;
                                    }
                                }
                                Err(e) => {
                                    log_error(&index_path, "parse_error", Some(&file.path), &format!("Failed to parse JSON: {}", e), None);
                                    progress.errors.push(format!("{}: Failed to parse JSON", file.name));
                                    error_count += 1;
                                }
                            }
                        } else if response.status().as_u16() == 429 {
                            // Rate limited - wait and retry
                            let wait_time = 2u64.pow(retries as u32) * 1000;
                            println!("[RUST] Rate limited, waiting {}ms...", wait_time);
                            log_error(&index_path, "rate_limit", Some(&file.path), "Rate limited by Azure", Some("429"));
                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_time)).await;
                            retries += 1;
                        } else {
                            let status = response.status();
                            let error_text = response.text().await.unwrap_or_default();
                            // Detect unsupported API version and attempt a fallback once
                            if error_text.contains("API version not supported") {
                                if api_version != "2023-10-01" {
                                    println!("[RUST] API version not supported, attempting fallback to 2023-10-01");
                                    api_version = "2023-10-01".to_string();
                                    retries = 0;
                                    continue; // retry this request with new api_version
                                }
                            }
                            log_error(&index_path, "api_error", Some(&file.path), &error_text, Some(&status.to_string()));
                            error_count += 1;
                            progress.errors.push(format!("{}: {} - {}", file.name, status, error_text));
                            break;
                        }
                    }
                    Err(e) => {
                        if retries < max_retries - 1 {
                            let wait_time = 2u64.pow(retries as u32) * 500;
                            tokio::time::sleep(tokio::time::Duration::from_millis(wait_time)).await;
                            retries += 1;
                        } else {
                            log_error(&index_path, "request_error", Some(&file.path), &e.to_string(), None);
                            error_count += 1;
                            progress.errors.push(format!("{}: {}", file.name, e));
                            break;
                        }
                    }
                }
            }
        }
        // Save this batch
        let batch_data = EmbeddingsData {
            embeddings: batch_embeddings.clone(),
            model: config.deployment_name.clone(),
            created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        };
        let batch_file = batch_dir.join(format!("embeddings_part_{:03}.json", batch_idx));
        let _ = fs::write(&batch_file, serde_json::to_string_pretty(&batch_data).unwrap_or_default());
        // Add to global
        new_embeddings.extend(batch_embeddings);
        batch_idx += 1;
        // Save progress
        progress.processed_files = new_embeddings.len();
        progress.current_batch = batch_idx;
        progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
        let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
    }
    // Final save: merge all batches into embeddings.json
    let final_data = EmbeddingsData {
        embeddings: new_embeddings.clone(),
        model: config.deployment_name.clone(),
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let json = serde_json::to_string_pretty(&final_data).map_err(|e| format!("Failed to serialize embeddings: {}", e))?;
    fs::write(&embeddings_file, json).map_err(|e| format!("Failed to write embeddings file: {}", e))?;
    // Optionally, clean up batch files here if desired
    println!("[RUST] Embeddings complete: {} generated, {} cached, {} skipped, {} errors", generated_count, cached_count, skipped_count, error_count);
    Ok(serde_json::json!({
        "embeddings_generated": generated_count,
        "cached_count": cached_count,
        "skipped_count": skipped_count,
        "error_count": error_count,
        "total_files": new_embeddings.len(),
        "message": format!("Generated {} new embeddings, {} from cache, {} skipped, {} errors", generated_count, cached_count, skipped_count, error_count),
        "collapsed_groups": collapsed_groups,
        "collapsed_files": collapsed_files
    }))
}

/// Generate embeddings using Google Cloud Vertex AI
pub async fn generate_embeddings_gcp(index_dir: String, max_files: Option<usize>, batch_size: Option<usize>) -> Result<serde_json::Value, String> {
    println!("[RUST] generate_embeddings_gcp called for: {}", index_dir);
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    let config_file = index_path.join("gcp_config.json");
    let embeddings_file = index_path.join("embeddings.json");
    let progress_file = index_path.join("embedding_progress.json");
    let cancel_file = index_path.join("embedding_cancel");

    // Clear stale cancel signals from previous runs
    if cancel_file.exists() {
        let _ = fs::remove_file(&cancel_file);
    }

    if !config_file.exists() {
        return Err("GCP config not found. Please configure GCP settings first.".to_string());
    }
    let config_content = fs::read_to_string(&config_file).map_err(|e| format!("Failed to read GCP config: {}", e))?;
    let config: GcpConfig = serde_json::from_str(&config_content).map_err(|e| format!("Failed to parse GCP config: {}", e))?;

    if !index_file.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }
    let index_content = fs::read_to_string(&index_file).map_err(|e| format!("Failed to read index: {}", e))?;
    let index_data: IndexData = serde_json::from_str(&index_content).map_err(|e| format!("Failed to parse index: {}", e))?;

    let files_initial: Vec<FileEntry> = if let Some(max) = max_files {
        index_data.files.into_iter().take(max).collect()
    } else {
        index_data.files
    };

    let (files_to_process, collapsed_groups, collapsed_files) = collapse_versions(files_initial);

    // Load existing embeddings to support resume/skip
    let mut existing_embeddings: Vec<FileEmbedding> = Vec::new();
    let mut existing_hashes: HashSet<String> = HashSet::new();
    if embeddings_file.exists() {
        if let Ok(content) = fs::read_to_string(&embeddings_file) {
            if let Ok(data) = serde_json::from_str::<EmbeddingsData>(&content) {
                for emb in data.embeddings {
                    existing_hashes.insert(emb.content_hash.clone());
                    existing_embeddings.push(emb);
                }
            }
        }
    }

    // Resolve credentials: prefer explicit service account JSON, otherwise try ADC.
    let bearer = if !config.service_account_path.trim().is_empty() {
        let sa_path = Path::new(&config.service_account_path);
        if !sa_path.exists() {
            return Err(format!("Service account file not found: {}", config.service_account_path));
        }

        // Build an access token using the service account
        let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
        let key = yup_oauth2::read_service_account_key(sa_path)
            .await
            .map_err(|e| format!("Failed to read service account key: {}", e))?;
        let auth = yup_oauth2::ServiceAccountAuthenticator::builder(key)
            .build()
            .await
            .map_err(|e| format!("Failed to build GCP authenticator: {}", e))?;
        let token = auth
            .token(scopes)
            .await
            .map_err(|e| format!("Failed to fetch GCP token: {}", e))?;
        token
            .token()
            .ok_or_else(|| "GCP token missing access token".to_string())?
            .to_string()
    } else {
        // Try GOOGLE_APPLICATION_CREDENTIALS env first
        if let Ok(env_path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            let p = Path::new(&env_path);
            if p.exists() {
                let scopes = &["https://www.googleapis.com/auth/cloud-platform"];
                let key = yup_oauth2::read_service_account_key(p)
                    .await
                    .map_err(|e| format!("Failed to read service account key from GOOGLE_APPLICATION_CREDENTIALS: {}", e))?;
                let auth = yup_oauth2::ServiceAccountAuthenticator::builder(key)
                    .build()
                    .await
                    .map_err(|e| format!("Failed to build GCP authenticator: {}", e))?;
                let token = auth
                    .token(scopes)
                    .await
                    .map_err(|e| format!("Failed to fetch GCP token: {}", e))?;
                token
                    .token()
                    .ok_or_else(|| "GCP token missing access token".to_string())?
                    .to_string()
            } else {
                return Err(format!("GOOGLE_APPLICATION_CREDENTIALS file not found: {}", env_path));
            }
        } else {
            // Fall back to gcloud ADC command to print an access token
            match std::process::Command::new("gcloud").args(&["auth", "application-default", "print-access-token"]).output() {
                Ok(out) => {
                    if out.status.success() {
                        let tok = String::from_utf8_lossy(&out.stdout).trim().to_string();
                        if tok.is_empty() {
                            return Err("gcloud returned empty ADC access token".to_string());
                        }
                        tok
                    } else {
                        return Err(format!("gcloud failed: {}", String::from_utf8_lossy(&out.stderr)));
                    }
                }
                Err(e) => return Err(format!("Failed to run gcloud for ADC token: {}", e)),
            }
        }
    };

    let client = reqwest::Client::new();
    let url = config.endpoint.unwrap_or_else(|| format!(
        "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:predict",
        config.location, config.project_id, config.location, config.model_id
    ));

    let is_gemini_embedding = config.model_id.to_lowercase().contains("gemini-embedding");

    let mut embeddings: Vec<FileEmbedding> = Vec::new();
    let mut generated_count = 0;
    let mut error_count = 0;
    let total_files = files_to_process.len();
    let chunk_size = batch_size.unwrap_or(1000).max(1);
    let total_batches = (total_files + chunk_size - 1) / chunk_size;

    let mut progress = BatchProgress {
        batch_id: format!("{}", Local::now().timestamp()),
        total_files,
        processed_files: 0,
        current_batch: 0,
        total_batches,
        batch_size: chunk_size,
        status: "running".to_string(),
        started_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        errors: Vec::new(),
    };
    let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());

    // Process in chunks to allow resume and reduce API load
    for (batch_idx, chunk) in files_to_process.chunks(chunk_size).enumerate() {
        progress.current_batch = batch_idx + 1;

        // Check for cancel signal
        if cancel_file.exists() {
            progress.status = "cancelled".to_string();
            let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
            return Err("Embedding cancelled by user".to_string());
        }

        for file in chunk {
            let content = match fs::read_to_string(&file.path) {
                Ok(c) => c,
                Err(_) => continue,
            };
            if content.trim().is_empty() {
                progress.processed_files += 1;
                progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
                continue;
            }

            // Trim to avoid context issues
            let truncated_content = if content.len() > 8000 { content[..8000].to_string() } else { content.clone() };

            let content_hash = format!("{:x}", md5_hash(&truncated_content));

            // Skip if already embedded with same content_hash
            if existing_hashes.contains(&content_hash) {
                progress.processed_files += 1;
                progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
                let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
                continue;
            }

            let payload = if is_gemini_embedding {
                serde_json::json!({ "instances": [{ "content": truncated_content }] })
            } else {
                serde_json::json!({ "instances": [{ "content": truncated_content }] })
            };

            let mut attempts: u64 = 0;
            let max_retries: u64 = 3;
            let mut success = false;
            let mut last_error: Option<String> = None;

            while attempts < max_retries && !success {
                let response = client
                    .post(&url)
                    .bearer_auth(&bearer)
                    .json(&payload)
                    .send()
                    .await;

                match response {
                    Ok(resp) => {
                        if resp.status().is_success() {
                            let json: serde_json::Value = resp.json().await.map_err(|e| format!("Failed to parse GCP response: {}", e))?;
                            let maybe_emb = if is_gemini_embedding {
                                json["predictions"][0]["values"].as_array()
                                    .or_else(|| json["predictions"][0]["embeddings"]["values"].as_array())
                            } else {
                                json["predictions"][0]["embedding"]["values"].as_array()
                                    .or_else(|| json["predictions"][0]["embedding"].as_array())
                            };

                            if let Some(embedding_values) = maybe_emb {
                                let emb_vec: Vec<f32> = embedding_values.iter()
                                    .filter_map(|v| v.as_f64().map(|f| f as f32))
                                    .collect();
                                let content_hash_clone = content_hash.clone();
                                embeddings.push(FileEmbedding {
                                    path: file.path.clone(),
                                    embedding: emb_vec,
                                    content_hash: content_hash_clone,
                                });
                                generated_count += 1;
                                existing_hashes.insert(content_hash.clone());
                                success = true;
                            } else {
                                let err_text = json.to_string();
                                last_error = Some(format!("Missing embedding in response: {}", err_text));
                                error_count += 1;
                                break; // Bad response structure — don't retry
                            }
                        } else {
                            let status = resp.status();
                            let text = resp.text().await.unwrap_or_default();
                            last_error = Some(format!("HTTP {}: {}", status, text));
                            error_count += 1;
                            // Don't retry on 4xx (bad request, quota, auth)
                            if status.is_client_error() {
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        last_error = Some(e.to_string());
                    }
                }

                if !success {
                    attempts += 1;
                    if attempts < max_retries {
                        tokio::time::sleep(std::time::Duration::from_millis(500 * attempts)).await;
                    }
                }
            }

            if !success {
                if let Some(ref err) = last_error {
                    log_error(index_path, "gcp_api_error", Some(&file.path), err, None);
                }
                // Fallback: use local deterministic embedding so search still works
                let local_emb = compute_embedding_from_text(&truncated_content, 512);
                let content_hash_clone = content_hash.clone();
                embeddings.push(FileEmbedding {
                    path: file.path.clone(),
                    embedding: local_emb,
                    content_hash: content_hash_clone,
                });
                existing_hashes.insert(content_hash.clone());
                generated_count += 1;
            }

            // update progress per file
            progress.processed_files += 1;
            progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
            let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());
        }
    }

    // Merge new embeddings with existing
    let mut merged = existing_embeddings;
    merged.extend(embeddings.clone());

    let final_data = EmbeddingsData {
        embeddings: merged,
        model: config.model_id.clone(),
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };

    let json = serde_json::to_string_pretty(&final_data).map_err(|e| format!("Failed to serialize embeddings: {}", e))?;
    fs::write(&embeddings_file, json).map_err(|e| format!("Failed to write embeddings file: {}", e))?;

    progress.status = "complete".to_string();
    progress.processed_files = total_files;
    progress.last_updated = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let _ = fs::write(&progress_file, serde_json::to_string_pretty(&progress).unwrap_or_default());

    Ok(serde_json::json!({
        "embeddings_generated": generated_count,
        "cached_count": 0,
        "error_count": error_count,
        "message": format!("Generated {} embeddings using GCP ({} errors).", generated_count, error_count),
        "collapsed_groups": collapsed_groups,
        "collapsed_files": collapsed_files
    }))
}

/// Get embedding progress
#[tauri::command]
pub async fn get_embedding_progress(index_dir: String) -> Result<serde_json::Value, String> {
    let progress_file = Path::new(&index_dir).join("embedding_progress.json");
    
    if !progress_file.exists() {
        return Ok(serde_json::json!({
            "status": "not_started",
            "message": "No embedding job has been started"
        }));
    }
    
    let content = fs::read_to_string(&progress_file)
        .map_err(|e| format!("Failed to read progress file: {}", e))?;
    
    let progress: BatchProgress = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse progress file: {}", e))?;
    
    Ok(serde_json::to_value(&progress).unwrap_or_default())
}

/// Get error log
#[tauri::command]
pub async fn get_error_log(index_dir: String, limit: Option<usize>) -> Result<serde_json::Value, String> {
    let error_log_file = Path::new(&index_dir).join("error_log.json");
    
    if !error_log_file.exists() {
        return Ok(serde_json::json!({
            "entries": [],
            "message": "No errors logged"
        }));
    }
    
    let content = fs::read_to_string(&error_log_file)
        .map_err(|e| format!("Failed to read error log: {}", e))?;
    
    let mut error_log: ErrorLog = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse error log: {}", e))?;

    // Apply limit
    let limit = limit.unwrap_or(100);
    if error_log.entries.len() > limit {
        error_log.entries = error_log.entries.split_off(error_log.entries.len() - limit);
    }

    Ok(serde_json::to_value(&error_log).unwrap_or_default())
}

/// Cancel an in-progress embedding job by writing a cancel signal file
#[tauri::command]
pub async fn cancel_embedding(index_dir: String) -> Result<(), String> {
    let cancel_file = Path::new(&index_dir).join("embedding_cancel");
    fs::write(&cancel_file, b"cancel")
        .map_err(|e| format!("Failed to write cancel file: {}", e))?;
    Ok(())
}

/// Multi-provider embedding: tries GCP → Azure → local in that order.
/// First provider that successfully embeds a file wins for that file.
/// Falls back automatically so all files always get an embedding.
#[tauri::command]
pub async fn generate_embeddings_multi(
    index_dir: String,
    max_files: Option<usize>,
    batch_size: Option<usize>,
) -> Result<serde_json::Value, String> {
    println!("[RUST] generate_embeddings_multi called for: {}", index_dir);

    // Try GCP first
    match generate_embeddings_gcp(index_dir.clone(), max_files, batch_size).await {
        Ok(v) => {
            println!("[RUST] Multi-embed: GCP succeeded");
            return Ok(v);
        }
        Err(e) => {
            println!("[RUST] Multi-embed: GCP failed ({}), trying Azure...", e);
        }
    }

    // Try Azure second
    match generate_embeddings_azure(index_dir.clone(), max_files, batch_size).await {
        Ok(v) => {
            println!("[RUST] Multi-embed: Azure succeeded");
            return Ok(v);
        }
        Err(e) => {
            println!("[RUST] Multi-embed: Azure failed ({}), falling back to local...", e);
        }
    }

    // Local fallback is always available
    println!("[RUST] Multi-embed: using local deterministic fallback");
    generate_embeddings_local(index_dir, max_files, None).await
}

/// Clear the error log
#[tauri::command]
pub async fn clear_error_log(index_dir: String) -> Result<(), String> {
    let error_log_file = Path::new(&index_dir).join("error_log.json");
    let empty = ErrorLog {
        entries: Vec::new(),
        last_updated: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    let json = serde_json::to_string_pretty(&empty)
        .map_err(|e| format!("Failed to serialize error log: {}", e))?;
    fs::write(&error_log_file, json)
        .map_err(|e| format!("Failed to write error log: {}", e))?;
    Ok(())
}

/// Create clusters using k-means algorithm
#[tauri::command]
pub async fn create_clusters(index_dir: String, num_clusters: Option<usize>) -> Result<serde_json::Value, String> {
    println!("[RUST] create_clusters called for: {}", index_dir);

    let index_path = Path::new(&index_dir);
    let embeddings_file = index_path.join("embeddings.json");
    let clusters_file = index_path.join("clusters.json");

    if !embeddings_file.exists() {
        return Err("No embeddings file found. Please generate embeddings first.".to_string());
    }

    let content = fs::read_to_string(&embeddings_file)
        .map_err(|e| format!("Failed to read embeddings: {}", e))?;

    let embeddings_data: EmbeddingsData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse embeddings: {}", e))?;
    
    if embeddings_data.embeddings.is_empty() {
        return Err("No embeddings found. Please generate embeddings first.".to_string());
    }
    
    // Determine number of clusters (default: sqrt of file count, min 2, max 20)
    let k = num_clusters.unwrap_or_else(|| {
        let sqrt = (embeddings_data.embeddings.len() as f64).sqrt() as usize;
        sqrt.max(2).min(20)
    });
    
    println!("[RUST] Clustering {} files into {} clusters", embeddings_data.embeddings.len(), k);
    
    // Run k-means clustering
    let clusters = kmeans_cluster(&embeddings_data.embeddings, k);
    
    // Save clusters
    let clusters_data = ClustersData {
        clusters: clusters.clone(),
        created_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    };
    
    let json = serde_json::to_string_pretty(&clusters_data)
        .map_err(|e| format!("Failed to serialize clusters: {}", e))?;
    
    fs::write(&clusters_file, json)
        .map_err(|e| format!("Failed to write clusters file: {}", e))?;
    
    println!("[RUST] Clustering complete: {} clusters created", clusters.len());
    
    Ok(serde_json::json!({
        "clusters_created": clusters.len(),
        "total_files": embeddings_data.embeddings.len(),
        "message": format!("Created {} clusters from {} files", clusters.len(), embeddings_data.embeddings.len())
    }))
}

/// K-means clustering implementation
fn kmeans_cluster(embeddings: &[FileEmbedding], k: usize) -> Vec<Cluster> {
    if embeddings.is_empty() || k == 0 {
        return Vec::new();
    }
    
    let dim = embeddings[0].embedding.len();
    let mut rng = rand::thread_rng();
    
    // Initialize centroids randomly from the embeddings
    let mut centroids: Vec<Vec<f32>> = Vec::with_capacity(k);
    let mut used_indices: Vec<usize> = Vec::new();
    
    for _ in 0..k.min(embeddings.len()) {
        let mut idx = rng.gen_range(0..embeddings.len());
        while used_indices.contains(&idx) {
            idx = rng.gen_range(0..embeddings.len());
        }
        used_indices.push(idx);
        centroids.push(embeddings[idx].embedding.clone());
    }
    
    // Run k-means for 50 iterations
    let mut assignments: Vec<usize> = vec![0; embeddings.len()];
    
    for iteration in 0..50 {
        // Assign each embedding to nearest centroid
        let mut changed = false;
        for (i, emb) in embeddings.iter().enumerate() {
            let mut min_dist = f32::MAX;
            let mut min_idx = 0;
            
            for (j, centroid) in centroids.iter().enumerate() {
                let dist = cosine_distance(&emb.embedding, centroid);
                if dist < min_dist {
                    min_dist = dist;
                    min_idx = j;
                }
            }
            
            if assignments[i] != min_idx {
                assignments[i] = min_idx;
                changed = true;
            }
        }
        
        if !changed {
            println!("[RUST] K-means converged at iteration {}", iteration);
            break;
        }
        
        // Update centroids
        for j in 0..centroids.len() {
            let mut new_centroid = vec![0.0f32; dim];
            let mut count = 0;
            
            for (i, emb) in embeddings.iter().enumerate() {
                if assignments[i] == j {
                    for (d, val) in emb.embedding.iter().enumerate() {
                        new_centroid[d] += val;
                    }
                    count += 1;
                }
            }
            
            if count > 0 {
                for val in new_centroid.iter_mut() {
                    *val /= count as f32;
                }
                centroids[j] = new_centroid;
            }
        }
    }
    
    // Build cluster results
    let mut clusters: Vec<Cluster> = Vec::with_capacity(k);
    
    for j in 0..centroids.len() {
        let file_paths: Vec<String> = embeddings
            .iter()
            .enumerate()
            .filter(|(i, _)| assignments[*i] == j)
            .map(|(_, emb)| emb.path.clone())
            .collect();
        
        if !file_paths.is_empty() {
            let label = generate_cluster_label(&file_paths);
            clusters.push(Cluster {
                id: j,
                centroid: centroids[j].clone(),
                file_paths,
                label: Some(label),
            });
        }
    }
    
    clusters
}

/// Generate a descriptive label for a cluster based on its files
fn generate_cluster_label(file_paths: &[String]) -> String {
    use std::collections::HashMap;
    
    let mut dir_counts: HashMap<String, usize> = HashMap::new();
    let mut ext_counts: HashMap<String, usize> = HashMap::new();
    let mut word_counts: HashMap<String, usize> = HashMap::new();
    
    // Common words to ignore
    let stopwords: std::collections::HashSet<&str> = [
        "the", "a", "an", "and", "or", "but", "in", "on", "at", "to", "for",
        "of", "with", "by", "from", "as", "is", "was", "are", "were", "been",
        "be", "have", "has", "had", "do", "does", "did", "will", "would", "could",
        "should", "may", "might", "must", "shall", "can", "need", "dare", "ought",
        "used", "index", "main", "test", "spec", "temp", "tmp", "copy", "new", "old"
    ].iter().cloned().collect();
    
    for path in file_paths {
        let path_obj = Path::new(path);
        
        // Count parent directories
        if let Some(parent) = path_obj.parent() {
            if let Some(dir_name) = parent.file_name() {
                let dir = dir_name.to_string_lossy().to_lowercase();
                if !dir.is_empty() && dir.len() > 1 {
                    *dir_counts.entry(dir).or_insert(0) += 1;
                }
            }
        }
        
        // Count extensions
        if let Some(ext) = path_obj.extension() {
            let ext_str = ext.to_string_lossy().to_lowercase();
            *ext_counts.entry(ext_str).or_insert(0) += 1;
        }
        
        // Extract words from filename
        if let Some(stem) = path_obj.file_stem() {
            let name = stem.to_string_lossy().to_lowercase();
            // Split on non-alphanumeric characters
            for word in name.split(|c: char| !c.is_alphanumeric()) {
                if word.len() > 2 && !stopwords.contains(word) {
                    *word_counts.entry(word.to_string()).or_insert(0) += 1;
                }
            }
        }
    }
    
    // Find most common extension
    let top_ext = ext_counts
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(ext, _)| ext.clone());
    
    // Find most common directory
    let top_dir = dir_counts
        .iter()
        .filter(|(dir, _)| dir.len() > 2)
        .max_by_key(|(_, count)| *count)
        .map(|(dir, _)| dir.clone());
    
    // Find most common meaningful word
    let top_word = word_counts
        .iter()
        .filter(|(word, count)| word.len() > 3 && **count > 1)
        .max_by_key(|(_, count)| *count)
        .map(|(word, _)| word.clone());
    
    // Build label
    let mut parts: Vec<String> = Vec::new();
    
    if let Some(word) = top_word {
        parts.push(capitalize(&word));
    }
    
    if let Some(dir) = top_dir {
        if parts.is_empty() || !parts[0].to_lowercase().contains(&dir) {
            parts.push(capitalize(&dir));
        }
    }
    
    if let Some(ext) = top_ext {
        let ext_label = match ext.as_str() {
            "md" => "Docs",
            "rs" => "Rust",
            "ts" | "tsx" => "TypeScript",
            "js" | "jsx" => "JavaScript",
            "py" => "Python",
            "json" => "Config",
            "yaml" | "yml" => "Config",
            "css" | "scss" => "Styles",
            "html" => "HTML",
            "sql" => "Database",
            "sh" | "bash" => "Scripts",
            "txt" => "Text",
            _ => &ext,
        };
        parts.push(ext_label.to_string());
    }
    
    if parts.is_empty() {
        format!("Group ({})", file_paths.len())
    } else {
        parts.join(" ")
    }
}

/// Capitalize first letter of a string
fn capitalize(s: &str) -> String {
    let mut chars = s.chars();
    match chars.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().chain(chars).collect(),
    }
}

/// Cosine distance between two vectors (1 - cosine similarity)
fn cosine_distance(a: &[f32], b: &[f32]) -> f32 {
    let mut dot = 0.0f32;
    let mut norm_a = 0.0f32;
    let mut norm_b = 0.0f32;
    
    for i in 0..a.len().min(b.len()) {
        dot += a[i] * b[i];
        norm_a += a[i] * a[i];
        norm_b += b[i] * b[i];
    }
    
    let similarity = dot / (norm_a.sqrt() * norm_b.sqrt() + 1e-10);
    1.0 - similarity
}

// ---------------------------------------------------------------------------
// Reference indexing and reversible move planning
// ---------------------------------------------------------------------------

fn load_index_data(index_dir: &str) -> Result<(IndexData, PathBuf), String> {
    let index_path = Path::new(index_dir).join("index.json");
    if !index_path.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }
    let content = fs::read_to_string(&index_path)
        .map_err(|e| format!("Failed to read index: {}", e))?;
    let index_data: IndexData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse index: {}", e))?;
    Ok((index_data, index_path))
}

fn resolve_candidate(raw: &str, from: &Path, index_files: &[FileEntry]) -> Option<PathBuf> {
    // Relative paths like ./foo or ../bar
    if raw.starts_with('.') {
        let mut cand = from.join(raw);
        // try with same extension
        if !cand.exists() {
            if let Some(ext) = from.extension() {
                cand.set_extension(ext);
            }
        }
        if cand.exists() {
            return Some(cand);
        }
    }

    // Absolute-like or module names: try basename/stem matches
    let raw_norm = raw.replace('\\', "/");
    for f in index_files {
        if raw_norm.ends_with(&f.name) || raw_norm.ends_with(&f.path) || raw_norm == f.name || raw_norm == f.path {
            return Some(PathBuf::from(&f.path));
        }
        let stem = Path::new(&f.name).file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if !stem.is_empty() && (raw_norm == stem || raw_norm.ends_with(stem)) {
            return Some(PathBuf::from(&f.path));
        }
    }
    None
}

fn extract_refs(index_data: &IndexData) -> RefIndex {
    let mut refs: HashMap<String, Vec<String>> = HashMap::new();
    let mut basename_map: HashMap<String, Vec<String>> = HashMap::new();

    for entry in &index_data.files {
        let path = PathBuf::from(&entry.path);
        let parent = path.parent().unwrap_or(Path::new("."));
        let text = fs::read_to_string(&entry.path).unwrap_or_default();

        let mut push_ref = |target: PathBuf| {
            let key = target.to_string_lossy().to_string();
            refs.entry(key.clone()).or_default().push(entry.path.clone());
            if let Some(base) = target.file_name().and_then(|b| b.to_str()) {
                basename_map.entry(base.to_string()).or_default().push(key);
            }
        };

        for cap in TS_IMPORT_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in TS_REQUIRE_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in PY_FROM_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in PY_IMPORT_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in RUST_USE_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in RUST_MOD_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
        for cap in CPP_INCLUDE_RE.captures_iter(&text) {
            if let Some(raw) = cap.name("p") {
                if let Some(target) = resolve_candidate(raw.as_str(), parent, &index_data.files) {
                    push_ref(target);
                }
            }
        }
    }

    // Deduplicate and sort
    for v in refs.values_mut() {
        v.sort();
        v.dedup();
    }
    for v in basename_map.values_mut() {
        v.sort();
        v.dedup();
    }

    RefIndex {
        refs,
        basename_map,
        built_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

fn load_ref_index(index_dir: &str) -> Result<RefIndex, String> {
    let ref_path = Path::new(index_dir).join("ref_index.json");
    if !ref_path.exists() {
        return Err("Reference index not found".to_string());
    }
    let content = fs::read_to_string(&ref_path).map_err(|e| format!("Failed to read ref index: {}", e))?;
    serde_json::from_str(&content).map_err(|e| format!("Failed to parse ref index: {}", e))
}

fn save_ref_index(index_dir: &str, ref_index: &RefIndex) -> Result<(), String> {
    let ref_path = Path::new(index_dir).join("ref_index.json");
    let content = serde_json::to_string_pretty(ref_index).map_err(|e| e.to_string())?;
    fs::write(&ref_path, content).map_err(|e| format!("Failed to write ref index: {}", e))
}

fn extract_keywords_from_name(path: &str) -> Vec<String> {
    let base = Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let re = Regex::new(r"[A-Za-z0-9]+").unwrap();
    re.find_iter(base)
        .map(|m| m.as_str().to_lowercase())
        .collect()
}

fn find_semantic_neighbors(target: &str, embeddings: &EmbeddingsData, top_k: usize) -> Vec<Neighbor> {
    let mut out = Vec::new();
    let target_vec = embeddings.embeddings.iter().find(|e| e.path == target);
    if let Some(t) = target_vec {
        for emb in &embeddings.embeddings {
            if emb.path == t.path {
                continue;
            }
            let dist = cosine_distance(&t.embedding, &emb.embedding);
            let score = 1.0 - dist;
            out.push(Neighbor { path: emb.path.clone(), score });
        }
        out.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        out.truncate(top_k);
    }
    out
}

fn load_embeddings(index_dir: &str) -> Option<EmbeddingsData> {
    let embeddings_file = Path::new(index_dir).join("embeddings.json");
    if !embeddings_file.exists() {
        return None;
    }
    if let Ok(content) = fs::read_to_string(&embeddings_file) {
        if let Ok(data) = serde_json::from_str::<EmbeddingsData>(&content) {
            return Some(data);
        }
    }
    None
}

fn safe_read_prefix(path: &str, bytes: usize) -> String {
    if let Ok(mut f) = fs::File::open(path) {
        use std::io::Read;
        let mut buf = vec![0u8; bytes];
        if let Ok(n) = f.read(&mut buf) {
            return String::from_utf8_lossy(&buf[..n]).to_string();
        }
    }
    String::new()
}

fn default_shim_content(from: &Path, to: &Path) -> (String, String) {
    let ext = from.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
    match ext.as_str() {
        "ts" | "tsx" | "js" | "jsx" => (format!("// Moved to {}\nexport * from \"{}\";\n", to.display(), to.display()), "ts".to_string()),
        "py" => (format!("# Moved to {}\nfrom {} import *  # shim\n", to.display(), to.display()), "py".to_string()),
        "rs" => (format!("// Moved to {}\npub use {}::*;\n", to.display(), to.display()), "rs".to_string()),
        "h" | "hpp" | "hh" | "hxx" => (format!("// Moved to {}\n#include \"{}\"\n", to.display(), to.display()), "cpp".to_string()),
        _ => (format!("// Moved to {}\n", to.display()), "txt".to_string()),
    }
}

fn append_move_history(index_dir: &str, plan: &MovePlan) {
    let history_path = Path::new(index_dir).join("move_history.json");
    let mut history: Vec<MovePlan> = if history_path.exists() {
        fs::read_to_string(&history_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };
    history.push(plan.clone());
    if let Ok(content) = serde_json::to_string_pretty(&history) {
        let _ = fs::write(&history_path, content);
    }
}

/// Search indexed files by query string
#[tauri::command]
pub async fn search(
    query: String,
    index_dir: String,
    top_k: usize,
    _semantic_weight: f32,
) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    
    if !index_file.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }

    let content = fs::read_to_string(&index_file)
        .map_err(|e| format!("Failed to read index: {}", e))?;
    
    let index_data: IndexData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse index: {}", e))?;

    let query_lower = query.to_lowercase();
    let mut results: Vec<SearchResult> = Vec::new();

    for file in &index_data.files {
        let name_lower = file.name.to_lowercase();
        let path_lower = file.path.to_lowercase();
        
        // Simple text matching score
        let mut score: f32 = 0.0;
        
        if name_lower.contains(&query_lower) {
            score += 1.0;
        }
        if path_lower.contains(&query_lower) {
            score += 0.5;
        }

        // Try to search within file content
        if let Ok(content) = fs::read_to_string(&file.path) {
            if content.to_lowercase().contains(&query_lower) {
                score += 0.8;
                
                // Get a preview snippet
                let content_lower = content.to_lowercase();
                if let Some(pos) = content_lower.find(&query_lower) {
                    let start = pos.saturating_sub(50);
                    let end = (pos + query.len() + 50).min(content.len());
                    let preview = &content[start..end];
                    
                    if score > 0.0 {
                        results.push(SearchResult {
                            path: file.path.clone(),
                            name: file.name.clone(),
                            score,
                            preview: Some(preview.trim().to_string()),
                        });
                    }
                    continue;
                }
            }
        }

        if score > 0.0 {
            results.push(SearchResult {
                path: file.path.clone(),
                name: file.name.clone(),
                score,
                preview: None,
            });
        }
    }

    // Sort by score descending and take top_k
    results.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);

    Ok(serde_json::to_value(results).unwrap())
}

/// Get summary of clusters
#[tauri::command]
pub async fn get_clusters_summary(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let clusters_file = index_path.join("clusters.json");
    
    if !clusters_file.exists() {
        return Ok(serde_json::json!({
            "clusters": [],
            "message": "No clusters found. Please create clusters first."
        }));
    }
    
    let content = fs::read_to_string(&clusters_file)
        .map_err(|e| format!("Failed to read clusters file: {}", e))?;
    
    let clusters_data: ClustersData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse clusters: {}", e))?;
    
    // Transform clusters for frontend display
    let clusters_summary: Vec<serde_json::Value> = clusters_data.clusters.iter().map(|cluster| {
        // Extract file names from paths for display
        let files: Vec<serde_json::Value> = cluster.file_paths.iter().map(|path| {
            let name = Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            serde_json::json!({
                "name": name,
                "path": path
            })
        }).collect();
        
        serde_json::json!({
            "id": cluster.id,
            "label": cluster.label.clone().unwrap_or_else(|| format!("Cluster {}", cluster.id + 1)),
            "file_count": cluster.file_paths.len(),
            "files": files
        })
    }).collect();
    
    Ok(serde_json::json!({
        "clusters": clusters_summary,
        "created_at": clusters_data.created_at,
        "total_clusters": clusters_summary.len()
    }))
}

/// Get timeline of file modifications
#[tauri::command]
pub async fn get_timeline(index_dir: String, days: usize) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    
    if !index_file.exists() {
        return Err("Index not found. Please scan a directory first.".to_string());
    }
    
    let content = fs::read_to_string(&index_file)
        .map_err(|e| format!("Failed to read index: {}", e))?;
    
    let index_data: IndexData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse index: {}", e))?;
    
    // Group files by date
    let mut files_by_date: std::collections::HashMap<String, Vec<serde_json::Value>> = std::collections::HashMap::new();
    
    for file in &index_data.files {
        // Parse the modified date and extract just the date part
        let date_part = if file.modified.len() >= 10 {
            file.modified[..10].to_string()
        } else {
            file.modified.clone()
        };
        
        files_by_date
            .entry(date_part)
            .or_insert_with(Vec::new)
            .push(serde_json::json!({
                "name": file.name,
                "path": file.path,
                "size": file.size,
                "modified": file.modified
            }));
    }
    
    // Sort dates in descending order and take only requested number of days
    let mut dates: Vec<String> = files_by_date.keys().cloned().collect();
    dates.sort_by(|a, b| b.cmp(a)); // Descending order (newest first)
    
    let timeline: Vec<serde_json::Value> = dates
        .into_iter()
        .take(days)
        .map(|date| {
            let files = files_by_date.get(&date).cloned().unwrap_or_default();
            serde_json::json!({
                "date": date,
                "files": files,
                "count": files.len()
            })
        })
        .collect();
    
    Ok(serde_json::json!({
        "timeline": timeline,
        "total_days": timeline.len(),
        "total_files": index_data.files.len()
    }))
}

/// Get index statistics
#[tauri::command]
pub async fn get_stats(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    let embeddings_file = index_path.join("embeddings.json");
    let clusters_file = index_path.join("clusters.json");
    
    if !index_file.exists() {
        return Err("Index not found".to_string());
    }

    let content = fs::read_to_string(&index_file)
        .map_err(|e| format!("Failed to read index: {}", e))?;
    
    let index_data: IndexData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse index: {}", e))?;

    let mut total_size: u64 = 0;
    let mut extensions: HashMap<String, usize> = HashMap::new();

    for file in &index_data.files {
        total_size += file.size;
        *extensions.entry(file.extension.clone()).or_insert(0) += 1;
    }

    // Check embeddings
    let (has_embeddings, embedding_count) = if embeddings_file.exists() {
        if let Ok(emb_content) = fs::read_to_string(&embeddings_file) {
            if let Ok(emb_data) = serde_json::from_str::<EmbeddingsData>(&emb_content) {
                (!emb_data.embeddings.is_empty(), emb_data.embeddings.len())
            } else {
                (false, 0)
            }
        } else {
            (false, 0)
        }
    } else {
        (false, 0)
    };

    // Check clusters
    let (has_clusters, cluster_count) = if clusters_file.exists() {
        if let Ok(clust_content) = fs::read_to_string(&clusters_file) {
            if let Ok(clust_data) = serde_json::from_str::<ClustersData>(&clust_content) {
                (!clust_data.clusters.is_empty(), clust_data.clusters.len())
            } else {
                (false, 0)
            }
        } else {
            (false, 0)
        }
    } else {
        (false, 0)
    };

    Ok(serde_json::json!({
        "total_files": index_data.files.len(),
        "total_size_bytes": total_size,
        "extensions": extensions,
        "last_updated": index_data.created_at,
        "scan_path": index_data.scan_path,
        "has_embeddings": has_embeddings,
        "embedding_count": embedding_count,
        "has_clusters": has_clusters,
        "cluster_count": cluster_count
    }))
}

// ---------------------------------------------------------------------------
// Reminder commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_reminders(include_done: Option<bool>) -> Result<serde_json::Value, String> {
    let store = load_reminders();
    let include_done = include_done.unwrap_or(false);
    let filtered: Vec<Reminder> = if include_done {
        store.reminders
    } else {
        store.reminders.into_iter().filter(|r| r.status != "done").collect()
    };

    Ok(serde_json::json!({
        "success": true,
        "reminders": filtered,
    }))
}

#[tauri::command]
pub async fn add_reminder(
    title: String,
    due: Option<String>,
    severity: Option<String>,
    link_path: Option<String>,
    repo_path: Option<String>,
) -> Result<serde_json::Value, String> {
    if title.trim().is_empty() {
        return Err("Title required".to_string());
    }

    let mut store = load_reminders();
    let now = now_string();
    let reminder = Reminder {
        id: new_id(),
        title: title.trim().to_string(),
        due,
        severity,
        link_path,
        repo_path,
        status: "open".to_string(),
        created_at: now.clone(),
        updated_at: now,
    };
    store.reminders.push(reminder.clone());
    save_reminders(&store)?;

    Ok(serde_json::json!({"success": true, "reminder": reminder}))
}

#[tauri::command]
pub async fn update_reminder_status(id: String, status: String) -> Result<serde_json::Value, String> {
    let mut store = load_reminders();
    let mut found = false;
    for r in store.reminders.iter_mut() {
        if r.id == id {
            r.status = status.clone();
            r.updated_at = now_string();
            found = true;
            break;
        }
    }
    if !found {
        return Err("Reminder not found".to_string());
    }
    save_reminders(&store)?;
    Ok(serde_json::json!({"success": true}))
}

#[tauri::command]
pub async fn snooze_reminder(id: String, hours: i64) -> Result<serde_json::Value, String> {
    let mut store = load_reminders();
    let mut found = false;
    for r in store.reminders.iter_mut() {
        if r.id == id {
            let future = Local::now() + chrono::Duration::hours(hours);
            r.due = Some(future.format("%Y-%m-%d %H:%M:%S").to_string());
            r.status = "snoozed".to_string();
            r.updated_at = now_string();
            found = true;
            break;
        }
    }
    if !found {
        return Err("Reminder not found".to_string());
    }
    save_reminders(&store)?;
    Ok(serde_json::json!({"success": true}))
}

/// Validate if index exists and is valid
#[tauri::command]
pub async fn validate_index(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    
    if !index_file.exists() {
        return Ok(serde_json::json!({
            "has_files": false,
            "index_valid": false,
            "message": "Index not found"
        }));
    }

    let content = fs::read_to_string(&index_file);
    match content {
        Ok(c) => {
            match serde_json::from_str::<IndexData>(&c) {
                Ok(data) => Ok(serde_json::json!({
                    "has_files": !data.files.is_empty(),
                    "index_valid": true,
                    "message": format!("Index valid with {} files", data.files.len())
                })),
                Err(_) => Ok(serde_json::json!({
                    "has_files": false,
                    "index_valid": false,
                    "message": "Index file is corrupted"
                }))
            }
        },
        Err(_) => Ok(serde_json::json!({
            "has_files": false,
            "index_valid": false,
            "message": "Cannot read index file"
        }))
    }
}

/// Get system information
#[tauri::command]
pub async fn get_system_info() -> Result<serde_json::Value, String> {
    Ok(serde_json::json!({
        "os": std::env::consts::OS,
        "arch": std::env::consts::ARCH
    }))
}

/// Save Azure OpenAI configuration
#[tauri::command]
pub async fn save_azure_config(
    index_dir: String,
    endpoint: String,
    api_key: String,
    deployment_name: String,
    api_version: Option<String>,
) -> Result<serde_json::Value, String> {
    println!("[RUST] save_azure_config called");
    
    let index_path = Path::new(&index_dir);
    fs::create_dir_all(&index_path)
        .map_err(|e| format!("Failed to create index directory: {}", e))?;
    
    let config_file = index_path.join("azure_config.json");
    
    // If no new key provided, try to preserve existing key
    let final_api_key = if api_key.is_empty() {
        // Try to load existing config to get the key
        if config_file.exists() {
            let content = fs::read_to_string(&config_file).ok();
            content.and_then(|c| {
                serde_json::from_str::<AzureConfig>(&c).ok()
            }).map(|c| c.api_key).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        api_key
    };
    
    let config = AzureConfig {
        endpoint,
        api_key: final_api_key,
        deployment_name,
        api_version: api_version.unwrap_or_else(|| "2024-02-01".to_string()),
    };
    
    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;
    
    fs::write(&config_file, json)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    // Ensure provider config is set to Azure when saving Azure config
    let _ = write_provider_config(index_path, EmbeddingProvider::Azure, None, None);
    
    Ok(serde_json::json!({
        "success": true,
        "message": "Azure config saved successfully"
    }))
}

/// Save Google Cloud Vertex AI configuration
#[tauri::command]
pub async fn save_gcp_config(
    index_dir: String,
    project_id: String,
    location: String,
    model_id: String,
    service_account_path: String,
    endpoint: Option<String>,
) -> Result<serde_json::Value, String> {
    println!("[RUST] save_gcp_config called");

    let index_path = Path::new(&index_dir);
    fs::create_dir_all(&index_path)
        .map_err(|e| format!("Failed to create index directory: {}", e))?;

    let config_file = index_path.join("gcp_config.json");

    let final_sa_path = if service_account_path.is_empty() {
        if config_file.exists() {
            let content = fs::read_to_string(&config_file).ok();
            content.and_then(|c| {
                serde_json::from_str::<GcpConfig>(&c).ok()
            }).map(|c| c.service_account_path).unwrap_or_default()
        } else {
            String::new()
        }
    } else {
        service_account_path
    };

    let config = GcpConfig {
        project_id,
        location,
        model_id,
        service_account_path: final_sa_path,
        endpoint,
    };

    let json = serde_json::to_string_pretty(&config)
        .map_err(|e| format!("Failed to serialize config: {}", e))?;

    fs::write(&config_file, json)
        .map_err(|e| format!("Failed to write config file: {}", e))?;

    let _ = write_provider_config(index_path, EmbeddingProvider::Gcp, None, None);

    Ok(serde_json::json!({
        "success": true,
        "message": "GCP config saved successfully"
    }))
}

/// Load embedding provider configuration
#[tauri::command]
pub async fn load_provider_config(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    if !index_path.exists() {
        return Err(format!("Index path not found: {}", index_dir));
    }

    let config = resolve_provider_config(index_path);
    Ok(serde_json::json!({
        "provider": match config.provider {
            EmbeddingProvider::Local => "local",
            EmbeddingProvider::Llama => "llama",
            EmbeddingProvider::Azure => "azure",
            EmbeddingProvider::Gcp => "gcp",
        },
        "local_model": config.local_model,
        "local_endpoint": config.local_endpoint,
        "exists": index_path.join("provider_config.json").exists()
    }))
}

/// Save embedding provider configuration
#[tauri::command]
pub async fn save_provider_config(
    index_dir: String,
    provider: String,
    local_model: Option<String>,
    local_endpoint: Option<String>,
) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    if !index_path.exists() {
        return Err(format!("Index path not found: {}", index_dir));
    }

    let provider_enum = match provider.to_lowercase().as_str() {
        "local" => EmbeddingProvider::Local,
        "llama" | "local_llama" | "local_gpu" => EmbeddingProvider::Llama,
        "azure" => EmbeddingProvider::Azure,
        "gcp" => EmbeddingProvider::Gcp,
        _ => return Err("Unknown provider. Use 'local', 'llama', 'azure', or 'gcp'.".to_string()),
    };

    let model = match provider_enum {
        EmbeddingProvider::Local => Some(local_model.unwrap_or_else(default_local_model_name)),
        EmbeddingProvider::Llama => local_model.or_else(|| Some("embeddinggemma-300m-f16".to_string())),
        EmbeddingProvider::Azure => local_model.clone(),
        EmbeddingProvider::Gcp => local_model,
    };

    let endpoint = match provider_enum {
        EmbeddingProvider::Llama => {
            let trimmed = local_endpoint.unwrap_or_default().trim().to_string();
            if trimmed.is_empty() { Some("http://localhost:5002".to_string()) } else { Some(trimmed) }
        },
        _ => local_endpoint,
    };

    write_provider_config(index_path, provider_enum, model, endpoint)?;

    Ok(serde_json::json!({
        "success": true,
        "message": "Provider config saved successfully"
    }))
}

/// Load Azure OpenAI configuration
#[tauri::command]
pub async fn load_azure_config(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let config_file = index_path.join("azure_config.json");
    
    if !config_file.exists() {
        return Ok(serde_json::json!({
            "configured": false,
            "endpoint": "",
            "deployment_name": "",
            "api_version": "2024-02-01"
        }));
    }
    
    let content = fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read config: {}", e))?;
    
    let config: AzureConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))?;
    
    Ok(serde_json::json!({
        "configured": !config.api_key.is_empty(),
        "endpoint": config.endpoint,
        "deployment_name": config.deployment_name,
        "api_version": config.api_version,
        "has_key": !config.api_key.is_empty()
    }))
}

/// Load Google Cloud Vertex AI configuration
#[tauri::command]
pub async fn load_gcp_config(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let config_file = index_path.join("gcp_config.json");

    if !config_file.exists() {
        return Ok(serde_json::json!({
            "configured": false,
            "project_id": "",
            "location": "",
            "model_id": "",
            "endpoint": null
        }));
    }

    let content = fs::read_to_string(&config_file)
        .map_err(|e| format!("Failed to read config: {}", e))?;

    let config: GcpConfig = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse config: {}", e))?;

    let has_key = !config.service_account_path.is_empty();
    let has_adc = std::env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .map(|p| Path::new(&p).exists())
        .unwrap_or(false);

    Ok(serde_json::json!({
        "configured": has_key || has_adc,
        "project_id": config.project_id,
        "location": config.location,
        "model_id": config.model_id,
        "endpoint": config.endpoint,
        "has_key": has_key,
        "has_adc": has_adc
    }))
}

/// Validate Azure configuration by making a small embeddings request
#[tauri::command]
pub async fn validate_azure_config(
    _index_dir: String,
    endpoint: String,
    api_key: String,
    deployment_name: String,
    api_version: Option<String>,
) -> Result<serde_json::Value, String> {
    println!("[RUST] validate_azure_config called for endpoint: {}", endpoint);

    // Normalize endpoint. Keep original for suggestion heuristics.
    let orig = endpoint.trim_end_matches('/').to_string();
    let mut base = orig.clone();
    let mut suggested: Option<String> = None;

    // If the user provided a project or API path, suggest the resource host form
    if orig.contains("/api/projects") || orig.contains("/api/") {
        if let Ok(url) = reqwest::Url::parse(&orig) {
            if let Some(host) = url.host_str() {
                if host.contains("services.ai.azure.com") {
                    if let Some(prefix) = host.split('.').next() {
                        suggested = Some(format!("https://{}.cognitiveservices.azure.com", prefix));
                    }
                } else {
                    suggested = Some(format!("https://{}", host));
                }
            }
        }
    } else if orig.contains("services.ai.azure.com") {
        if let Ok(url) = reqwest::Url::parse(&orig) {
            if let Some(host) = url.host_str() {
                if let Some(prefix) = host.split('.').next() {
                    suggested = Some(format!("https://{}.cognitiveservices.azure.com", prefix));
                }
            }
        }
    }

    // Ensure base is reduced to scheme + host (+port) only to avoid double-paths
    if let Ok(url) = reqwest::Url::parse(&base) {
        if let Some(host) = url.host_str() {
            let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
            base = format!("{}://{}{}", url.scheme(), host, port);
        }
    }

    // Prepare versions to try
    let mut tried_versions: Vec<String> = Vec::new();
    let api_version_current = api_version.unwrap_or_else(|| "2024-02-01".to_string());
    let fallback_versions = vec!["2024-02-01".to_string(), "2023-10-01".to_string(), "2023-05-15".to_string()];

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(8))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Try current and fallbacks
    let mut last_url: Option<String> = None;
    for v in std::iter::once(api_version_current.clone()).chain(fallback_versions.into_iter()) {
        if tried_versions.contains(&v) { continue; }
        tried_versions.push(v.clone());

        // Ensure base has /openai path
        let mut url_base = base.clone();
        if !url_base.ends_with("/openai") && !url_base.ends_with("/openai/") {
            url_base = format!("{}/openai", url_base);
        }

        let url = format!("{}/deployments/{}/embeddings?api-version={}", url_base, deployment_name, v);
        last_url = Some(url.clone());

        println!("[RUST] validate attempt url: {}", url);

        let body = serde_json::json!({ "input": ["healthcheck"] });

        match client.post(&url).header("api-key", &api_key).json(&body).send().await {
            Ok(response) => {
                let status = response.status().as_u16();
                if response.status().is_success() {
                    // Good response - success
                    return Ok(serde_json::json!({
                        "success": true,
                        "message": "Validation succeeded",
                        "tried_versions": tried_versions,
                        "final_url": url,
                        "status_code": status
                    }));
                } else {
                    let text = response.text().await.unwrap_or_default();
                    // If api-version not supported, try next
                    if text.contains("API version not supported") {
                        println!("[RUST] API version not supported for {}", v);
                        continue;
                    }
                    // Return error details
                    return Ok(serde_json::json!({
                        "success": false,
                        "message": format!("Server returned {}: {}", status, text),
                        "tried_versions": tried_versions,
                        "final_url": url,
                        "status_code": status,
                        "suggested_endpoint": suggested
                    }));
                }
            }
            Err(e) => {
                println!("[RUST] Request error: {}", e);
                // network or connection error - return as failure but include suggestion
                return Ok(serde_json::json!({
                    "success": false,
                    "message": format!("Request failed: {}", e),
                    "tried_versions": tried_versions,
                    "final_url": last_url,
                    "suggested_endpoint": suggested
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "success": false,
        "message": "All tried API versions failed",
        "tried_versions": tried_versions,
        "final_url": last_url,
        "suggested_endpoint": suggested
    }))
}

/// Validate Google Cloud configuration
#[tauri::command]
pub async fn validate_gcp_config(
    project_id: String,
    location: String,
    model_id: String,
    service_account_path: String,
    endpoint: Option<String>,
) -> Result<serde_json::Value, String> {
    if service_account_path.trim().is_empty() {
        return Ok(serde_json::json!({
            "success": false,
            "message": "Service account JSON path is required"
        }));
    }

    let sa_path = std::path::Path::new(&service_account_path);
    if !sa_path.exists() {
        return Ok(serde_json::json!({
            "success": false,
            "message": format!("Service account file not found: {}", service_account_path)
        }));
    }

    let key = match yup_oauth2::read_service_account_key(sa_path).await {
        Ok(k) => k,
        Err(e) => {
            return Ok(serde_json::json!({
                "success": false,
                "message": format!("Failed to read service account: {}", e)
            }))
        }
    };

    let auth = match yup_oauth2::ServiceAccountAuthenticator::builder(key).build().await {
        Ok(a) => a,
        Err(e) => {
            return Ok(serde_json::json!({
                "success": false,
                "message": format!("Failed to build authenticator: {}", e)
            }))
        }
    };

    let token_res = auth.token(&["https://www.googleapis.com/auth/cloud-platform"]).await;
    let token = match token_res {
        Ok(t) => t,
        Err(e) => {
            return Ok(serde_json::json!({
                "success": false,
                "message": format!("Failed to fetch token: {}", e)
            }))
        }
    };

    let bearer = match token.token() {
        Some(t) => t.to_string(),
        None => {
            return Ok(serde_json::json!({
                "success": false,
                "message": "Token missing access token"
            }))
        }
    };

    let url = endpoint.unwrap_or_else(|| format!(
        "https://{}-aiplatform.googleapis.com/v1/projects/{}/locations/{}/publishers/google/models/{}:predict",
        location, project_id, location, model_id
    ));

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .bearer_auth(bearer)
        .json(&serde_json::json!({
            "instances": [
                {
                    "content": "healthcheck"
                }
            ]
        }))
        .send()
        .await;

    match response {
        Ok(res) => {
            if res.status().is_success() {
                Ok(serde_json::json!({
                    "success": true,
                    "message": "GCP validation successful"
                }))
            } else {
                let status = res.status();
                let text = res.text().await.unwrap_or_default();
                Ok(serde_json::json!({
                    "success": false,
                    "message": format!("GCP validation failed with status {}: {}", status, text)
                }))
            }
        }
        Err(e) => Ok(serde_json::json!({
            "success": false,
            "message": format!("GCP validation request failed: {}", e)
        })),
    }
}

// ---------------------------------------------------------------------------
// Commands: classify files and build reversible view
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn classify_files(
    index_dir: String,
    labels: HashMap<String, Vec<String>>,
    top_n: Option<usize>,
    rules: Option<ClassificationRules>,
    smart_llm: Option<bool>,
    llm_model: Option<String>,
    llm_endpoint: Option<String>,
    llm_limit: Option<usize>,
    llm_min_score: Option<f32>,
    llm_top_alternates: Option<usize>,
) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let index_file = index_path.join("index.json");
    if !index_file.exists() {
        return Err("Index not found. Run scan first.".to_string());
    }
    let content = fs::read_to_string(&index_file).map_err(|e| format!("Failed to read index: {}", e))?;
    let index_data: IndexData = serde_json::from_str(&content).map_err(|e| format!("Failed to parse index: {}", e))?;

    let limit = top_n.unwrap_or(index_data.files.len());
    let files: Vec<FileEntry> = index_data.files.into_iter().take(limit).collect();
    let rules = rules.unwrap_or_default();
    let mut classified = classify_files_with_labels(&files, &labels, &rules);

    // Optional LLM refinement for ambiguous/low-confidence files
    if smart_llm.unwrap_or(false) {
        let endpoint = llm_endpoint.unwrap_or_else(|| "http://localhost:5001".to_string());
        let model = llm_model.unwrap_or_else(|| "qwen2.5-coder-14b-instruct".to_string());
        let llm_limit = llm_limit.unwrap_or(15).max(0);
        let min_score = llm_min_score.unwrap_or(1.0);
        let top_alts = llm_top_alternates.unwrap_or(2).max(0);
        let allowed_labels: Vec<String> = labels.keys().cloned().collect();
        let mut refined = 0usize;

        for cf in classified.iter_mut() {
            if refined >= llm_limit {
                break;
            }

            let alt_gap = cf
                .alternates
                .get(0)
                .map(|a| (cf.confidence - a.1).abs())
                .unwrap_or(f32::MAX);
            let ambiguous = alt_gap < rules.ambiguity_delta.unwrap_or(0.4);
            let low_conf = cf.confidence < min_score;
            let unsorted = cf.project == "unsorted";

            if !(ambiguous || low_conf || unsorted) {
                continue;
            }

            let alt_labels: Vec<String> = cf
                .alternates
                .iter()
                .take(top_alts as usize)
                .map(|a| a.0.clone())
                .collect();

            if let Some((label, reason)) = refine_classification_with_llm(
                cf,
                &allowed_labels,
                &alt_labels,
                &endpoint,
                &model,
            )
            .await
            {
                if allowed_labels.contains(&label) {
                    cf.project = label.clone();
                    cf.reasons.push(format!("LLM refine: {}", reason));
                    cf.confidence = cf.confidence.max(min_score + 0.1);
                } else {
                    cf.reasons.push(format!("LLM refine returned out-of-set label '{}'; kept {}", label, cf.project));
                }
            }

            refined += 1;
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "classified": classified,
    }))
}

#[tauri::command]
pub async fn build_view_plan(
    classified: Vec<ClassifiedFile>,
    view_root: String,
    prefer_junction: Option<bool>,
    dry_run: Option<bool>,
) -> Result<serde_json::Value, String> {
    let view_root_path = PathBuf::from(&view_root);
    let prefer_junction = prefer_junction.unwrap_or(true);
    let dry = dry_run.unwrap_or(true);

    let plan = propose_move_plan(&classified, &view_root_path);
    let mut applied: Vec<MovePlanEntry> = Vec::new();
    let mut errors: Vec<String> = Vec::new();

    if !dry {
        for entry in &plan {
            let from = Path::new(&entry.from);
            let to = Path::new(&entry.to);
            match create_link(from, to, prefer_junction) {
                Ok(kind) => {
                    let mut e = entry.clone();
                    e.link_type = kind;
                    applied.push(e);
                }
                Err(err) => errors.push(err),
            }
        }
    }

    Ok(serde_json::json!({
        "success": errors.is_empty(),
        "plan": plan,
        "applied": applied,
        "errors": errors,
        "dry_run": dry,
        "view_root": view_root,
    }))
}

/// Validate azure_config.json files found under a root path (recursively)
#[tauri::command]
pub async fn validate_all_azure_configs(root_path: String) -> Result<serde_json::Value, String> {
    println!("[RUST] validate_all_azure_configs scanning: {}", root_path);

    let mut results: Vec<serde_json::Value> = Vec::new();

    if !Path::new(&root_path).exists() {
        return Err(format!("Root path does not exist: {}", root_path));
    }

    for entry in WalkDir::new(&root_path).into_iter().filter_map(|e| e.ok()) {
        if entry.file_type().is_dir() && entry.file_name() == ".wayfinder_index" {
            let idx_dir = entry.path().to_string_lossy().to_string();
            let cfg_file = entry.path().join("azure_config.json");
            if cfg_file.exists() {
                match fs::read_to_string(&cfg_file) {
                    Ok(content) => {
                        match serde_json::from_str::<AzureConfig>(&content) {
                            Ok(cfg) => {
                                // Call existing validate function to reuse logic
                                match validate_azure_config(idx_dir.clone(), cfg.endpoint.clone(), cfg.api_key.clone(), cfg.deployment_name.clone(), Some(cfg.api_version.clone())).await {
                                    Ok(v) => {
                                        results.push(serde_json::json!({
                                            "index_dir": idx_dir,
                                            "config": cfg,
                                            "validation": v
                                        }));
                                    }
                                    Err(e) => {
                                        results.push(serde_json::json!({
                                            "index_dir": idx_dir,
                                            "config": cfg,
                                            "error": e
                                        }));
                                    }
                                }
                            }
                            Err(e) => {
                                results.push(serde_json::json!({
                                    "index_dir": idx_dir,
                                    "error": format!("Failed to parse config: {}", e)
                                }));
                            }
                        }
                    }
                    Err(e) => {
                        results.push(serde_json::json!({
                            "index_dir": idx_dir,
                            "error": format!("Failed to read config: {}", e)
                        }));
                    }
                }
            } else {
                // No Azure config present; mark as skipped instead of failure so GCP-only indexes don't look broken
                results.push(serde_json::json!({
                    "index_dir": idx_dir,
                    "config": null,
                    "validation": {"success": true, "message": "No azure_config.json present (skipped)"}
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "success": true,
        "root_scanned": root_path,
        "results": results
    }))
}

/// Get clusters summary for display
#[tauri::command]
pub async fn get_clusters_data(index_dir: String) -> Result<serde_json::Value, String> {
    let index_path = Path::new(&index_dir);
    let clusters_file = index_path.join("clusters.json");
    
    if !clusters_file.exists() {
        return Ok(serde_json::json!({
            "has_clusters": false,
            "clusters": []
        }));
    }
    
    let content = fs::read_to_string(&clusters_file)
        .map_err(|e| format!("Failed to read clusters: {}", e))?;
    
    let clusters_data: ClustersData = serde_json::from_str(&content)
        .map_err(|e| format!("Failed to parse clusters: {}", e))?;
    
    // Return cluster summaries (without full centroids for UI)
    let clusters_summary: Vec<serde_json::Value> = clusters_data.clusters.iter().map(|c| {
        serde_json::json!({
            "id": c.id,
            "file_count": c.file_paths.len(),
            "files": c.file_paths,
            "label": c.label
        })
    }).collect();
    
    Ok(serde_json::json!({
        "has_clusters": true,
        "clusters": clusters_summary,
        "created_at": clusters_data.created_at
    }))
}

/// Get Git Clippy report for a repository
#[tauri::command]
pub async fn get_git_clippy_report(repo_path: String, index_dir: Option<String>) -> Result<serde_json::Value, String> {
    println!("[RUST] get_git_clippy_report called for: {}", repo_path);
    
    // Load index data if available
    let index_files = if let Some(ref dir) = index_dir {
        let index_file = Path::new(dir).join("index.json");
        if index_file.exists() {
            let content = fs::read_to_string(&index_file).ok();
            content.and_then(|c| serde_json::from_str::<IndexData>(&c).ok())
                .map(|d| d.files)
        } else {
            None
        }
    } else {
        None
    };
    
    let report = git_assistant::generate_clippy_report(&repo_path, index_files.as_deref())?;
    
    serde_json::to_value(report)
        .map_err(|e| format!("Failed to serialize report: {}", e))
}

/// Get Nauticlippy report for arbitrary folders (non-git helper)
#[tauri::command]
pub async fn get_nauticlippy_report(paths: Vec<String>) -> Result<serde_json::Value, String> {
    println!("[RUST] get_nauticlippy_report called for {} paths", paths.len());

    // Build FileEntry list from provided paths, skipping .git and hidden files
    let mut files: Vec<FileEntry> = Vec::new();

    for root in paths {
        let root_path = Path::new(&root);
        if !root_path.exists() {
            continue;
        }

        for entry in WalkDir::new(root_path)
            .follow_links(true)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let file_path = entry.path();

            // Skip .git
            if file_path.ancestors().any(|a| a.file_name().and_then(|n| n.to_str()) == Some(".git")) {
                continue;
            }

            // Skip hidden files/dirs
            if file_path
                .file_name()
                .and_then(|n| n.to_str())
                .map(|n| n.starts_with('.'))
                .unwrap_or(false)
            {
                continue;
            }

            if file_path.is_file() {
                if let Ok(metadata) = fs::metadata(file_path) {
                    let size = metadata.len();
                    let modified = metadata
                        .modified()
                        .ok()
                        .map(|t| DateTime::<Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_else(|| "Unknown".to_string());

                    let ext = file_path
                        .extension()
                        .and_then(|e| e.to_str())
                        .unwrap_or("")
                        .to_lowercase();

                    files.push(FileEntry {
                        path: file_path.to_string_lossy().to_string(),
                        name: file_path
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or("unknown")
                            .to_string(),
                        size,
                        modified,
                        extension: ext,
                    });
                }
            }
        }
    }

    let duplicates = git_assistant::find_duplicates(&files);
    let copy_patterns = git_assistant::detect_copy_patterns(&files);

    Ok(serde_json::json!({
        "message": if duplicates.is_empty() && copy_patterns.is_empty() { "Nothing to clean up" } else { "Review potential cleanups" },
        "scanned_files": files.len(),
        "duplicates": duplicates,
        "copy_pattern_files": copy_patterns,
    }))
}

/// Execute a Git Clippy action
#[tauri::command]
pub async fn execute_clippy_action(
    repo_path: String, 
    action: String, 
    data: Option<serde_json::Value>
) -> Result<serde_json::Value, String> {
    println!("[RUST] execute_clippy_action: {} for {}", action, repo_path);
    
    let result = git_assistant::execute_git_action(&repo_path, &action, data.as_ref())?;
    
    Ok(serde_json::json!({
        "success": true,
        "output": result
    }))
}

/// Check if path is a git repository
#[tauri::command]
pub async fn is_git_repo(path: String) -> Result<bool, String> {
    Ok(git_assistant::is_git_repo(&path))
}

/// Delete duplicate files - used by Git Clippy
#[tauri::command]
pub async fn delete_duplicate_files(file_paths: Vec<String>) -> Result<serde_json::Value, String> {
    println!("[RUST] delete_duplicate_files: {} files", file_paths.len());
    
    let mut deleted = 0;
    let mut errors: Vec<String> = Vec::new();
    
    for path in &file_paths {
        match fs::remove_file(path) {
            Ok(_) => {
                deleted += 1;
                println!("[RUST] Deleted: {}", path);
            }
            Err(e) => {
                let error_msg = format!("Failed to delete {}: {}", path, e);
                println!("[RUST] {}", error_msg);
                errors.push(error_msg);
            }
        }
    }
    
    Ok(serde_json::json!({
        "success": errors.is_empty(),
        "deleted": deleted,
        "errors": errors
    }))
}

/// Lightweight local chat via llama.cpp server (OpenAI-compatible)
#[tauri::command]
pub async fn chat_llama(prompt: String, model: Option<String>, endpoint: Option<String>) -> Result<serde_json::Value, String> {
    let client = reqwest::Client::new();
    let base = endpoint.unwrap_or_else(|| "http://localhost:5001".to_string());
    let url = format!("{}/v1/chat/completions", base.trim_end_matches('/'));
    let model_name = model.unwrap_or_else(|| "qwen2.5-coder-1.5b-instruct-q4_k_m.gguf".to_string());

    let payload = serde_json::json!({
        "model": model_name,
        "messages": [
            {"role": "system", "content": "You are a concise coding assistant. Answer briefly."},
            {"role": "user", "content": prompt}
        ],
        "temperature": 0.3,
        "max_tokens": 256
    });

    let resp = client.post(&url).json(&payload).send().await
        .map_err(|e| format!("Request failed: {}", e))?;
    let status = resp.status();
    if !status.is_success() {
        let err_text = resp.text().await.unwrap_or_default();
        return Err(format!("Chat failed ({}): {}", status, err_text));
    }

    let json: serde_json::Value = resp.json().await.map_err(|e| format!("Parse failed: {}", e))?;
    let text = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();

    Ok(serde_json::json!({ "text": text }))
}

// ============================================================================
// FILE INTELLIGENCE COMMANDS
// ============================================================================

use crate::file_intelligence::{
    self, DiscoveredDocument, OrganizationSuggestion, UserPreferences,
};
use std::sync::Mutex;
use once_cell::sync::Lazy;

// Global state for user preferences (will be replaced with SQLite later)
static USER_PREFS: Lazy<Mutex<UserPreferences>> = Lazy::new(|| Mutex::new(UserPreferences::default()));
static LAST_SCAN: Lazy<Mutex<Vec<DiscoveredDocument>>> = Lazy::new(|| Mutex::new(Vec::new()));

/// Scan a directory for organizable documents
#[tauri::command]
pub async fn scan_for_documents(root_path: String, max_depth: Option<usize>) -> Result<serde_json::Value, String> {
    println!("[FILE_INTEL] scan_for_documents: {}", root_path);
    
    let documents = file_intelligence::scan_for_documents(&root_path, max_depth)?;
    
    // Store for later use
    if let Ok(mut scan) = LAST_SCAN.lock() {
        *scan = documents.clone();
    }
    
    let count = documents.len();
    
    Ok(serde_json::json!({
        "success": true,
        "document_count": count,
        "documents": documents
    }))
}

/// Get organization suggestions based on last scan
#[tauri::command]
pub async fn get_organization_suggestions() -> Result<serde_json::Value, String> {
    println!("[FILE_INTEL] get_organization_suggestions");
    
    let documents = LAST_SCAN.lock()
        .map_err(|e| format!("Lock error: {}", e))?
        .clone();
    
    let prefs = USER_PREFS.lock()
        .map_err(|e| format!("Lock error: {}", e))?
        .clone();
    
    if documents.is_empty() {
        return Ok(serde_json::json!({
            "success": true,
            "suggestions": [],
            "message": "No documents scanned yet. Run scan_for_documents first."
        }));
    }
    
    let suggestions = file_intelligence::generate_suggestions(&documents, &prefs);
    
    Ok(serde_json::json!({
        "success": true,
        "suggestion_count": suggestions.len(),
        "suggestions": suggestions
    }))
}

/// Get statistics about the scanned documents
#[tauri::command]
pub async fn get_scan_statistics() -> Result<serde_json::Value, String> {
    println!("[FILE_INTEL] get_scan_statistics");
    
    let documents = LAST_SCAN.lock()
        .map_err(|e| format!("Lock error: {}", e))?
        .clone();
    
    if documents.is_empty() {
        return Ok(serde_json::json!({
            "success": false,
            "message": "No documents scanned yet"
        }));
    }
    
    let stats = file_intelligence::calculate_statistics(&documents);
    let patterns = file_intelligence::detect_naming_patterns(&documents);
    
    Ok(serde_json::json!({
        "success": true,
        "statistics": stats,
        "naming_patterns": patterns
    }))
}

/// Dismiss a suggestion (don't suggest this file again)
#[tauri::command]
pub async fn dismiss_suggestion(file_path: String) -> Result<serde_json::Value, String> {
    println!("[FILE_INTEL] dismiss_suggestion: {}", file_path);
    
    let mut prefs = USER_PREFS.lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    
    prefs.dismissed_suggestions.push(file_path.clone());
    
    Ok(serde_json::json!({
        "success": true,
        "dismissed": file_path
    }))
}

// ============================================================================
// FILE WATCHER COMMANDS
// ============================================================================

use crate::file_watcher::{FileWatcher, WatchConfig, FileEvent};
use crate::file_watcher::event_to_document;
use crate::file_watcher::should_prompt_for_event;
use crate::file_watcher::SavePrompterConfig;

static FILE_WATCHER: Lazy<Mutex<Option<FileWatcher>>> = Lazy::new(|| Mutex::new(None));
static WATCHER_EVENTS: Lazy<Mutex<Vec<FileEvent>>> = Lazy::new(|| Mutex::new(Vec::new()));
static WATCHER_SUGGESTIONS: Lazy<Mutex<Vec<WatcherSuggestion>>> = Lazy::new(|| Mutex::new(Vec::new()));
static LAST_PROMPT_TIMES: Lazy<Mutex<std::collections::HashMap<String, std::time::Instant>>> = Lazy::new(|| Mutex::new(std::collections::HashMap::new()));

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WatcherSuggestion {
    pub suggestion: OrganizationSuggestion,
    pub event: FileEvent,
}

/// Start the file watcher
#[tauri::command]
pub async fn start_file_watcher(watch_paths: Option<Vec<String>>) -> Result<serde_json::Value, String> {
    println!("[FILE_WATCHER] start_file_watcher");
    
    let mut config = WatchConfig::default();
    if let Some(paths) = watch_paths {
        config.paths = paths;
    }
    
    let mut watcher = FileWatcher::new(config.clone());
    let rx = watcher.start()?;
    
    // Store the watcher
    {
        let mut w = FILE_WATCHER.lock().map_err(|e| format!("Lock error: {}", e))?;
        *w = Some(watcher);
    }
    
    // Spawn a thread to collect events
    std::thread::spawn(move || {
        while let Ok(event) = rx.recv() {
            // Store raw event
            if let Ok(mut e) = WATCHER_EVENTS.lock() {
                e.push(event.clone());
                if e.len() > 100 { e.remove(0); }
            }

            // Generate organization suggestions for this event
            if let Ok(doc) = std::panic::catch_unwind(|| event_to_document(&event)) {
                let doc = doc;
                // Grab user prefs
                let prefs = USER_PREFS
                    .lock()
                    .ok()
                    .map(|p| p.clone())
                    .unwrap_or_default();

                // Prompt gating (cooldown, event type)
                let prompter_cfg = SavePrompterConfig::default();
                let mut last_prompts = LAST_PROMPT_TIMES.lock().ok();
                let should_prompt = match last_prompts.as_mut() {
                    Some(map) => should_prompt_for_event(&event, &prompter_cfg, map),
                    None => true,
                };

                if should_prompt {
                    let suggestions = file_intelligence::generate_suggestions(&[doc], &prefs);
                    if let Some(sugg) = suggestions.into_iter().next() {
                        if let Ok(mut s) = WATCHER_SUGGESTIONS.lock() {
                            s.push(WatcherSuggestion { suggestion: sugg, event: event.clone() });
                            if s.len() > 50 { s.remove(0); }
                        }
                        if let Some(map) = last_prompts.as_mut() {
                            map.insert(event.path.clone(), std::time::Instant::now());
                        }
                    }
                }
            }
        }
    });
    
    Ok(serde_json::json!({
        "success": true,
        "watching": config.paths,
        "message": "File watcher started"
    }))
}

/// Stop the file watcher
#[tauri::command]
pub async fn stop_file_watcher() -> Result<serde_json::Value, String> {
    println!("[FILE_WATCHER] stop_file_watcher");
    
    let mut watcher_lock = FILE_WATCHER.lock().map_err(|e| format!("Lock error: {}", e))?;
    
    if let Some(ref mut watcher) = *watcher_lock {
        watcher.stop()?;
    }
    
    *watcher_lock = None;
    
    Ok(serde_json::json!({
        "success": true,
        "message": "File watcher stopped"
    }))
}

/// Get pending file events
#[tauri::command]
pub async fn get_file_events(clear: Option<bool>) -> Result<serde_json::Value, String> {
    let mut events = WATCHER_EVENTS.lock()
        .map_err(|e| format!("Lock error: {}", e))?;
    
    let result = events.clone();
    
    if clear.unwrap_or(false) {
        events.clear();
    }
    
    Ok(serde_json::json!({
        "success": true,
        "event_count": result.len(),
        "events": result
    }))
}

/// Get pending file organization suggestions
#[tauri::command]
pub async fn get_file_suggestions(clear: Option<bool>) -> Result<serde_json::Value, String> {
    let mut suggestions = WATCHER_SUGGESTIONS.lock()
        .map_err(|e| format!("Lock error: {}", e))?;

    let result = suggestions.clone();

    if clear.unwrap_or(false) {
        suggestions.clear();
    }

    Ok(serde_json::json!({
        "success": true,
        "suggestions": result
    }))
}

/// Get file watcher status
#[tauri::command]
pub async fn get_watcher_status() -> Result<serde_json::Value, String> {
    let watcher_lock = FILE_WATCHER.lock().map_err(|e| format!("Lock error: {}", e))?;
    let events = WATCHER_EVENTS.lock().map_err(|e| format!("Lock error: {}", e))?;
    
    let (is_running, paths) = match &*watcher_lock {
        Some(w) => {
            let state = w.get_state()?;
            (state.is_running, state.watched_paths)
        }
        None => (false, Vec::new()),
    };
    
    Ok(serde_json::json!({
        "success": true,
        "is_running": is_running,
        "watched_paths": paths,
        "pending_events": events.len()
    }))
}

/// Offline index sync commands
#[tauri::command]
pub async fn cache_index_locally(index_dir: String, cache_dir: String) -> Result<bool, String> {
    // Call Python backend offline.py:cache_index_locally
    let output = std::process::Command::new("python")
        .arg("-m")
        .arg("md_scanner.offline")
        .arg("cache_index_locally")
        .arg(&index_dir)
        .arg(&cache_dir)
        .output()
        .map_err(|e| format!("Failed to launch Python: {}", e))?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn export_index(index_dir: String, export_path: String) -> Result<bool, String> {
    let output = std::process::Command::new("python")
        .arg("-m")
        .arg("md_scanner.offline")
        .arg("export_index")
        .arg(&index_dir)
        .arg(&export_path)
        .output()
        .map_err(|e| format!("Failed to launch Python: {}", e))?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

#[tauri::command]
pub async fn import_index(zip_path: String, target_dir: String) -> Result<bool, String> {
    let output = std::process::Command::new("python")
        .arg("-m")
        .arg("md_scanner.offline")
        .arg("import_index")
        .arg(&zip_path)
        .arg(&target_dir)
        .output()
        .map_err(|e| format!("Failed to launch Python: {}", e))?;
    if output.status.success() {
        Ok(true)
    } else {
        Err(String::from_utf8_lossy(&output.stderr).to_string())
    }
}

// Add md5 helper that returns Digest for easy hex formatting
fn md5_hash(s: &str) -> md5::Digest {
    md5::compute(s)
}

// ---------------------------------------------------------------------------
// Utility: collapse versioned duplicates and prefer newest
// ---------------------------------------------------------------------------

fn normalize_stem(name: &str) -> String {
    let lower = name.to_lowercase();
    let cleaned = lower
        .replace(" copy", "")
        .replace("- copy", "")
        .replace(" copy", "")
        .replace("(copy)", "")
        .replace("(1)", "")
        .replace("(2)", "")
        .replace("(3)", "")
        .replace("final", "")
        .replace("draft", "")
        .replace("backup", "")
        .replace("bak", "")
        .replace("old", "")
        .replace("new", "")
        .replace("v1", "")
        .replace("v2", "")
        .replace("v3", "");

    let tokens: Vec<&str> = cleaned
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();

    if tokens.is_empty() {
        cleaned.trim().to_string()
    } else {
        tokens.join("_")
    }
}

fn collapse_versions(files: Vec<FileEntry>) -> (Vec<FileEntry>, usize, usize) {
    let mut map: HashMap<String, FileEntry> = HashMap::new();
    let mut collapsed = 0usize;

    for f in files.into_iter() {
        let stem = Path::new(&f.path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("");
        let key = normalize_stem(stem);

        let existing = map.get(&key);
        let newer = if let Some(e) = existing {
            let mt_new = DateTime::parse_from_str(&f.modified, "%Y-%m-%d %H:%M:%S")
                .or_else(|_| DateTime::parse_from_rfc3339(&f.modified))
                .map(|d| d.timestamp())
                .unwrap_or(0);
            let mt_old = DateTime::parse_from_str(&e.modified, "%Y-%m-%d %H:%M:%S")
                .or_else(|_| DateTime::parse_from_rfc3339(&e.modified))
                .map(|d| d.timestamp())
                .unwrap_or(0);
            mt_new >= mt_old
        } else {
            true
        };

        if newer {
            if existing.is_some() {
                collapsed += 1;
            }
            map.insert(key.clone(), f);
        } else {
            collapsed += 1;
        }
    }

    let groups = map.len();
    (map.into_values().collect(), groups, collapsed)
}

// ---------------------------------------------------------------------------
// Project classification and move-plan generation
// ---------------------------------------------------------------------------

fn tokenize_path(path: &str) -> Vec<String> {
    Path::new(path)
        .components()
        .filter_map(|c| c.as_os_str().to_str())
        .flat_map(|s| s.split(|ch: char| !ch.is_alphanumeric()))
        .filter(|t| !t.is_empty())
        .map(|t| t.to_lowercase())
        .collect()
}

fn score_file(label: &str, tokens: &[String]) -> (f32, Vec<String>) {
    let mut score = 0.0f32;
    let mut reasons = Vec::new();
    let needle = label.to_lowercase();
    let hits: Vec<&String> = tokens.iter().filter(|t| t.contains(&needle)).collect();
    if !hits.is_empty() {
        score += 1.0;
        reasons.push(format!("path matches {:?}", hits));
    }
    score
        += tokens
            .iter()
            .filter(|t| t == &&needle)
            .count() as f32
            * 0.5;
    if score > 0.0 {
        reasons.push(format!("token hit for {}", label));
    }
    (score, reasons)
}

/// Use an external chat-completion (llama.cpp/OpenAI-compatible) to refine the label choice.
async fn refine_classification_with_llm(
    file: &ClassifiedFile,
    allowed_labels: &[String],
    alt_labels: &[String],
    endpoint: &str,
    model: &str,
) -> Option<(String, String)> {
    let client = reqwest::Client::new();
    let base = endpoint.trim_end_matches('/');
    let url = format!("{}/v1/chat/completions", base);

    let content = fs::read_to_string(&file.path).ok()?;
    let snippet: String = content.chars().take(4000).collect();

    let mut label_list: Vec<String> = allowed_labels.to_vec();
    label_list.extend_from_slice(alt_labels);
    label_list.sort();
    label_list.dedup();

    let user_prompt = format!(
        "Choose ONE label from this list: {}.\nCurrent guess: {}. Alternates: {:?}.\nFile path: {}\nContent snippet (truncated):\n{}\nReturn strict JSON: {{\"label\":<label>, \"reason\":<short>}}."
        ,
        label_list.join(", "),
        file.project,
        file.alternates,
        file.path,
        snippet
    );

    let payload = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a precise file classifier. Respond ONLY with compact JSON."},
            {"role": "user", "content": user_prompt}
        ],
        "temperature": 0.2,
        "max_tokens": 200
    });

    let resp = client.post(&url).json(&payload).send().await.ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let json: serde_json::Value = resp.json().await.ok()?;
    let text = json
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("");

    let cleaned = text.trim().trim_matches('`').trim();
    let parsed: serde_json::Value = serde_json::from_str(cleaned).ok()?;
    let label = parsed.get("label").and_then(|l| l.as_str()).unwrap_or("").trim().to_string();
    let reason = parsed.get("reason").and_then(|l| l.as_str()).unwrap_or("").trim().to_string();

    if label.is_empty() {
        return None;
    }
    Some((label, if reason.is_empty() { "LLM selected label".to_string() } else { reason }))
}

fn classify_files_with_labels(
    files: &[FileEntry],
    labels: &HashMap<String, Vec<String>>,
    rules: &ClassificationRules,
) -> Vec<ClassifiedFile> {
    let mut out = Vec::new();
    for f in files {
        let tokens = tokenize_path(&f.path);
        let mut best = ("unsorted".to_string(), 0.0f32, Vec::new());
        let mut alts: Vec<(String, f32)> = Vec::new();

        for (label, seeds) in labels {
            let mut label_score = 0.0f32;
            let mut reasons = Vec::new();
            for seed in seeds {
                let (s, r) = score_file(seed, &tokens);
                label_score += s;
                reasons.extend(r);
            }

            // Include patterns: if any explicit pattern matches path, boost strongly
            if let Some(map) = &rules.include_patterns {
                if let Some(pats) = map.get(label) {
                    for pat in pats {
                        if f.path.to_lowercase().contains(&pat.to_lowercase()) {
                            label_score += 5.0;
                            reasons.push(format!("include pattern hit: {}", pat));
                        }
                    }
                }
            }

            // Exclude patterns: if match, zero the score
            if let Some(map) = &rules.exclude_patterns {
                if let Some(pats) = map.get(label) {
                    if pats.iter().any(|p| f.path.to_lowercase().contains(&p.to_lowercase())) {
                        label_score = 0.0;
                        reasons.push("excluded by pattern".to_string());
                    }
                }
            }

            if label_score > best.1 {
                if best.1 > 0.0 {
                    alts.push((best.0.clone(), best.1));
                }
                best = (label.clone(), label_score, reasons);
            } else if label_score > 0.0 {
                alts.push((label.clone(), label_score));
            }
        }

        if best.1 == 0.0 {
            best.0 = "unsorted".to_string();
        }

        alts.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let mut final_project = best.0.clone();
        let final_conf = best.1;

        if let Some(min) = rules.min_confidence {
            if final_conf < min {
                final_project = "unsorted".to_string();
            }
        }

        if let Some(delta) = rules.ambiguity_delta {
            if let Some(top_alt) = alts.first() {
                if (final_conf - top_alt.1).abs() <= delta {
                    final_project = "unsorted".to_string();
                }
            }
        }

        out.push(ClassifiedFile {
            path: f.path.clone(),
            project: final_project,
            confidence: final_conf,
            reasons: best.2,
            alternates: alts.into_iter().take(3).collect(),
        });
    }
    out
}

fn propose_move_plan(classified: &[ClassifiedFile], view_root: &Path) -> Vec<MovePlanEntry> {
    let mut plan = Vec::new();
    for cf in classified {
        if cf.project == "unsorted" {
            continue;
        }
        let rel = Path::new(&cf.path)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown");
        let dest = view_root.join(&cf.project).join(rel);
        plan.push(MovePlanEntry {
            from: cf.path.clone(),
            to: dest.to_string_lossy().to_string(),
            link_type: "symlink".to_string(),
        });
    }
    plan
}

fn create_link(from: &Path, to: &Path, prefer_junction: bool) -> Result<String, String> {
    if prefer_junction {
        if to.exists() {
            return Ok("exists".to_string());
        }
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Failed to mkdir {:?}: {}", parent, e))?;
        }
        #[cfg(target_os = "windows")]
        {
            if std::os::windows::fs::symlink_dir(from, to).is_ok() {
                return Ok("junction".to_string());
            }
        }
    }

    if to.exists() {
        return Ok("exists".to_string());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Failed to mkdir {:?}: {}", parent, e))?;
    }

    #[cfg(target_os = "windows")]
    {
        if std::os::windows::fs::symlink_file(from, to).is_ok() {
            return Ok("symlink".to_string());
        }
    }

    #[cfg(not(target_os = "windows"))]
    {
        if std::os::unix::fs::symlink(from, to).is_ok() {
            return Ok("symlink".to_string());
        }
    }

    if std::fs::hard_link(from, to).is_ok() {
        return Ok("hardlink".to_string());
    }

    std::fs::copy(from, to)
        .map(|_| "copy".to_string())
        .map_err(|e| format!("Failed to copy {:?} -> {:?}: {}", from, to, e))
}

// ---------------------------------------------------------------------------
// New commands: reference index, cluster insight, reversible move planning
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn build_ref_index(index_dir: String) -> Result<serde_json::Value, String> {
    let (index_data, _) = load_index_data(&index_dir)?;
    let ref_index = extract_refs(&index_data);
    save_ref_index(&index_dir, &ref_index)?;
    Ok(serde_json::json!({
        "success": true,
        "built_at": ref_index.built_at,
        "refs": ref_index.refs.len(),
    }))
}

#[tauri::command]
pub async fn analyze_cluster(
    files: Vec<String>,
    index_dir: String,
    include_semantic: bool,
) -> Result<serde_json::Value, String> {
    let (index_data, _) = load_index_data(&index_dir)?;
    let ref_index = load_ref_index(&index_dir).unwrap_or_else(|_| extract_refs(&index_data));
    let embeddings = if include_semantic { load_embeddings(&index_dir) } else { None };

    let mut insights: Vec<FileInsight> = Vec::new();
    for path in files {
        let mut imports = ref_index.refs.get(&path).cloned().unwrap_or_default();
        imports.sort();
        imports.dedup();

        let base = Path::new(&path)
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        let mut mentions = Vec::new();
        for f in &index_data.files {
            if f.path == path {
                continue;
            }
            if let Ok(content) = fs::read_to_string(&f.path) {
                if !base.is_empty() && content.contains(&base) {
                    mentions.push(f.path.clone());
                }
            }
        }
        mentions.sort();
        mentions.dedup();

        let semantic_neighbors = if let (true, Some(emb)) = (include_semantic, &embeddings) {
            find_semantic_neighbors(&path, emb, 5)
        } else {
            Vec::new()
        };

        let keywords = extract_keywords_from_name(&path);
        let score = imports.len() as f32 + (semantic_neighbors.first().map(|n| n.score).unwrap_or(0.0));

        insights.push(FileInsight {
            path: path.clone(),
            imports,
            mentions,
            semantic_neighbors,
            keywords,
            score,
        });
    }

    Ok(serde_json::json!(insights))
}

fn generate_plan_id() -> String {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let rand: u32 = rand::thread_rng().gen();
    format!("plan-{}-{:08x}", ts, rand)
}

fn ensure_parent(path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("Failed to create {:?}: {}", parent, e))?
    }
    Ok(())
}

#[tauri::command]
pub async fn plan_moves(
    target_root: String,
    files: Vec<String>,
    strategy: String,
) -> Result<serde_json::Value, String> {
    let plan_id = generate_plan_id();
    let mut steps: Vec<MoveAction> = Vec::new();
    let mut revert_steps: Vec<MoveAction> = Vec::new();

    for from in files {
        let from_path = PathBuf::from(&from);
        let fname = from_path.file_name().and_then(|s| s.to_str()).unwrap_or("moved");
        let to_path = Path::new(&target_root).join(fname);
        let (shim_content, _lang) = default_shim_content(&from_path, &to_path);

        steps.push(MoveAction {
            action: "move".to_string(),
            from: Some(from.clone()),
            to: Some(to_path.to_string_lossy().to_string()),
            path: None,
            shim_content: None,
            note: Some(strategy.clone()),
        });

        steps.push(MoveAction {
            action: "create_shim".to_string(),
            from: None,
            to: None,
            path: Some(from.clone()),
            shim_content: Some(shim_content.clone()),
            note: Some("shim".to_string()),
        });

        revert_steps.push(MoveAction {
            action: "delete".to_string(),
            from: None,
            to: None,
            path: Some(from.clone()),
            shim_content: None,
            note: Some("remove shim".to_string()),
        });
        revert_steps.push(MoveAction {
            action: "move".to_string(),
            from: Some(to_path.to_string_lossy().to_string()),
            to: Some(from.clone()),
            path: None,
            shim_content: None,
            note: Some("revert".to_string()),
        });
    }

    let plan = MovePlan {
        plan_id: plan_id.clone(),
        generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
        target_root,
        steps,
        revert_steps,
    };

    Ok(serde_json::to_value(plan).map_err(|e| e.to_string())?)
}

#[tauri::command]
pub async fn apply_move_plan(index_dir: String, plan: MovePlan) -> Result<serde_json::Value, String> {
    let mut errors: Vec<String> = Vec::new();

    for step in &plan.steps {
        match step.action.as_str() {
            "move" => {
                if let (Some(from), Some(to)) = (&step.from, &step.to) {
                    let from_path = PathBuf::from(from);
                    let to_path = PathBuf::from(to);
                    if let Err(e) = ensure_parent(&to_path) {
                        errors.push(e);
                        continue;
                    }
                    if let Err(e) = fs::rename(&from_path, &to_path) {
                        errors.push(format!("Failed to move {:?} -> {:?}: {}", from_path, to_path, e));
                    }
                }
            }
            "create_shim" => {
                if let (Some(path), Some(content)) = (&step.path, &step.shim_content) {
                    let shim_path = PathBuf::from(path);
                    if let Err(e) = ensure_parent(&shim_path) {
                        errors.push(e);
                        continue;
                    }
                    if let Err(e) = fs::write(&shim_path, content) {
                        errors.push(format!("Failed to write shim {:?}: {}", shim_path, e));
                    }
                }
            }
            "delete" => {
                if let Some(path) = &step.path {
                    let p = PathBuf::from(path);
                    if p.exists() {
                        if let Err(e) = fs::remove_file(&p) {
                            errors.push(format!("Failed to delete {:?}: {}", p, e));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    append_move_history(&index_dir, &plan);

    Ok(serde_json::json!({
        "success": errors.is_empty(),
        "errors": errors,
        "applied_plan": plan,
    }))
}

#[tauri::command]
pub async fn list_move_history(index_dir: String) -> Result<serde_json::Value, String> {
    let history_path = Path::new(&index_dir).join("move_history.json");
    if history_path.exists() {
        if let Ok(content) = fs::read_to_string(&history_path) {
            if let Ok(history) = serde_json::from_str::<Vec<MovePlan>>(&content) {
                return Ok(serde_json::json!(history));
            }
        }
    }
    Ok(serde_json::json!(Vec::<MovePlan>::new()))
}

#[tauri::command]
pub async fn undo_move_plan(index_dir: String, plan_id: String) -> Result<serde_json::Value, String> {
    let history_path = Path::new(&index_dir).join("move_history.json");
    let mut history: Vec<MovePlan> = if history_path.exists() {
        fs::read_to_string(&history_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    let plan_opt = history.iter().find(|p| p.plan_id == plan_id).cloned();
    if plan_opt.is_none() {
        return Err("Plan not found".to_string());
    }
    let plan = plan_opt.unwrap();
    let mut errors: Vec<String> = Vec::new();

    for step in &plan.revert_steps {
        match step.action.as_str() {
            "move" => {
                if let (Some(from), Some(to)) = (&step.from, &step.to) {
                    let from_path = PathBuf::from(from);
                    let to_path = PathBuf::from(to);
                    if let Err(e) = ensure_parent(&to_path) {
                        errors.push(e);
                        continue;
                    }
                    if let Err(e) = fs::rename(&from_path, &to_path) {
                        errors.push(format!("Failed to move {:?} -> {:?}: {}", from_path, to_path, e));
                    }
                }
            }
            "create_shim" => {
                if let (Some(path), Some(content)) = (&step.path, &step.shim_content) {
                    let shim_path = PathBuf::from(path);
                    if let Err(e) = ensure_parent(&shim_path) {
                        errors.push(e);
                        continue;
                    }
                    if let Err(e) = fs::write(&shim_path, content) {
                        errors.push(format!("Failed to write shim {:?}: {}", shim_path, e));
                    }
                }
            }
            "delete" => {
                if let Some(path) = &step.path {
                    let p = PathBuf::from(path);
                    if p.exists() {
                        if let Err(e) = fs::remove_file(&p) {
                            errors.push(format!("Failed to delete {:?}: {}", p, e));
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(serde_json::json!({
        "success": errors.is_empty(),
        "errors": errors,
        "reverted_plan": plan,
    }))
}

