use serde_json::Value;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;
use tracing::{info, warn};

/// Counter for think_harder calls per session. Resets on /clear.
static THINK_HARDER_COUNT: AtomicU32 = AtomicU32::new(0);
const THINK_HARDER_LIMIT: u32 = u32::MAX; // No limit — let it search as much as it needs

pub fn reset_think_counter() {
    THINK_HARDER_COUNT.store(0, Ordering::SeqCst);
}

/// Execute a tool by name with given arguments and project root.
pub async fn execute(name: &str, arguments: &Value, project_root: &Path) -> String {
    info!("Tool call: {} args={}", name, arguments);

    match name {
        "write_file" => { reset_think_counter(); write_file(arguments, project_root).await },
        "read_file" => { reset_think_counter(); read_file(arguments, project_root).await },
        "cargo_check" => { reset_think_counter(); cargo_check(arguments, project_root).await },
        "think_harder" => { reset_think_counter(); think_harder(arguments).await },
        "remember" => { reset_think_counter(); remember(arguments, project_root).await },
        "run_command" => run_command(arguments, project_root).await,
        "search_symbols" => symforge_proxy("search_symbols", arguments).await,
        "get_symbol" => symforge_proxy("get_symbol", arguments).await,
        "get_file_context" => symforge_proxy("get_file_context", arguments).await,
        "replace_symbol_body" => symforge_proxy("replace_symbol_body", arguments).await,
        "edit_within_symbol" => symforge_proxy("edit_within_symbol", arguments).await,
        "insert_symbol" => symforge_proxy("insert_symbol", arguments).await,
        "delete_symbol" => symforge_proxy("delete_symbol", arguments).await,
        "batch_edit" => symforge_proxy("batch_edit", arguments).await,
        "batch_rename" => symforge_proxy("batch_rename", arguments).await,
        "search_text" => search_text(arguments, project_root).await,
        "scan_region" => scan_region(arguments).await,
        "magnetic_dipole_detect" => magnetic_dipole_detect(arguments).await,
        "download_satellite_window" => download_satellite_window(arguments).await,
        "weather_window" => weather_window(arguments).await,
        "detection_health" => detection_health(arguments).await,
        "detection_scan" => detection_scan(arguments).await,
        "detection_poll" => detection_poll(arguments).await,
        "sat_mission" => sat_mission(arguments).await,
        "sat_read_mission_report" => sat_read_mission_report(arguments).await,
        _ => format!("Unknown tool: {}. Available tools: read_file, write_file, run_command, think_harder, cargo_check, remember, scan_region, magnetic_dipole_detect, download_satellite_window, weather_window, detection_health, detection_scan, detection_poll, sat_mission, sat_read_mission_report", name),
    }
}

/// Write a file to the project root. Creates parent directories if needed.
async fn write_file(args: &Value, project_root: &Path) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let full_path = resolve_path(path, project_root);
    
    // Create parent directories
    if let Some(parent) = full_path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return format!("Error creating directory {}: {}", parent.display(), e);
        }
    }
    
    match fs::File::create(&full_path) {
        Ok(mut file) => {
            match file.write_all(content.as_bytes()) {
                Ok(_) => {
                    info!("Written {} bytes to {}", content.len(), full_path.display());
                    format!("Successfully wrote {} ({} bytes)", full_path.display(), content.len())
                },
                Err(e) => format!("Error writing file: {}", e),
            }
        },
        Err(e) => format!("Error creating file: {}", e),
    }
}

/// Read a file, truncating at 3000 chars.
async fn read_file(args: &Value, project_root: &Path) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let full_path = resolve_path(path, project_root);
    
    match fs::read_to_string(&full_path) {
        Ok(contents) => {
            let truncated = truncate_output(&contents, 3000);
            format!("Contents of {}:\n{}", full_path.display(), truncated)
        },
        Err(e) => format!("Error reading file {}: {}", full_path.display(), e),
    }
}

/// Run cargo check on the given directory.
async fn cargo_check(args: &Value, project_root: &Path) -> String {
    let dir = match args.get("dir").and_then(|v| v.as_str()) {
        Some(d) => d,
        None => return "Error: 'dir' argument required".to_string(),
    };
    let full_dir = resolve_path(dir, project_root);
    
    info!("Running cargo check in {}", full_dir.display());
    
    let output = Command::new("cargo")
        .arg("check")
        .current_dir(&full_dir)
        .output()
        .await;
    
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                format!("Cargo check succeeded:\n{}", truncate_output(&stdout, 2000))
            } else {
                format!("Cargo check failed:\nSTDOUT:{}\nSTDERR:{}", 
                    truncate_output(&stdout, 1500), truncate_output(&stderr, 1500))
            }
        },
        Err(e) => format!("Failed to run cargo check: {}", e),
    }
}

/// Search nautivecs knowledge base + web (same contract as forge-v2).
async fn think_harder(args: &Value) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument required".to_string(),
    };

    let count = THINK_HARDER_COUNT.fetch_add(1, Ordering::SeqCst);
    if count >= THINK_HARDER_LIMIT {
        warn!("think_harder limit reached (count={})", count);
        return "Error: think_harder limit reached".to_string();
    }

    info!("think_harder search #{}: {}", count + 1, query);

    let client = reqwest::Client::new();
    let mut results = Vec::new();

    if let Ok(resp) = client
        .post(crate::knowledge::nautivecs_query_url())
        .json(&serde_json::json!({"query": query, "top_k": 3}))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        if let Ok(body) = resp.text().await {
            results.push(format!("[nautivecs]: {}", truncate_output(&body, 1500)));
        }
    } else {
        results.push("[nautivecs]: unavailable".to_string());
    }

    if let Ok(resp) = client
        .post(crate::knowledge::wso_url())
        .json(&serde_json::json!({"query": query, "max_results": 3}))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
    {
        if let Ok(body) = resp.text().await {
            results.push(format!("[web search]: {}", truncate_output(&body, 1500)));
        }
    }

    if results.is_empty() {
        "No results from knowledge base or web search.".to_string()
    } else {
        results.join("\n\n")
    }
}

/// Save a lesson to research_log and nautivecs /add.
async fn remember(args: &Value, project_root: &Path) -> String {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let tags = args.get("tags").and_then(|v| v.as_str()).unwrap_or("general");

    let log_path = resolve_path("research_log/lessons_learned.md", project_root);
    if let Some(parent) = log_path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    let entry = format!("\n## [{}] {}\n{}\n", tags, chrono_now(), content);
    let existing = fs::read_to_string(&log_path).unwrap_or_default();
    let log_ok = fs::write(&log_path, format!("{}{}", existing, entry)).is_ok();

    let client = reqwest::Client::new();
    let nautivecs_status = match client
        .post(crate::knowledge::nautivecs_add_url())
        .json(&serde_json::json!({
            "text": format!("[{}] {}", tags, content),
            "tags": tags,
            "source": "lessons_learned",
            "file_path": "research_log/lessons_learned.md",
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => " + indexed in nautivecs".to_string(),
        Ok(r) => format!(" (nautivecs {})", r.status()),
        Err(e) => format!(" (nautivecs unavailable: {})", e),
    };

    if log_ok {
        format!("Remembered (tags: {}){}", tags, nautivecs_status)
    } else {
        format!("Error writing {}", log_path.display())
    }
}

/// Execute a shell command.
async fn run_command(args: &Value, project_root: &Path) -> String {
    let cmd = match args.get("cmd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'cmd' argument required".to_string(),
    };
    
    info!("Executing command: {}", cmd);
    
    let output = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .current_dir(project_root)
        .output()
        .await;
    
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout).to_string();
            let stderr = String::from_utf8_lossy(&out.stderr).to_string();
            if out.status.success() {
                format!("Command succeeded:\nSTDOUT:{}\nSTDERR:{}", 
                    truncate_output(&stdout, 2000), truncate_output(&stderr, 1000))
            } else {
                format!("Command failed (exit code {:?}):\nSTDOUT:{}\nSTDERR:{}", 
                    out.status.code(), truncate_output(&stdout, 1500), truncate_output(&stderr, 1500))
            }
        },
        Err(e) => format!("Failed to execute command: {}", e),
    }
}

async fn symforge_proxy(tool: &str, args: &Value) -> String {
    let url = std::env::var("SYMFORGE_RPC_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:7788/rpc".to_string());
    let payload = serde_json::json!({
        "jsonrpc": "2.0",
        "id": format!("sf-{}", chrono_now()),
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": args
        }
    });
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building SymForge client: {}", e),
    };
    match client.post(&url).json(&payload).send().await {
        Ok(resp) => match resp.text().await {
            Ok(body) => truncate_output(&body, 6000),
            Err(e) => format!("SymForge response read error: {}", e),
        },
        Err(e) => format!(
            "SymForge unavailable at {} ({}). Set SYMFORGE_RPC_URL or start SymForge MCP server.",
            url, e
        ),
    }
}

async fn search_text(args: &Value, project_root: &Path) -> String {
    // Lightweight local fallback for SymForge search_text.
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) if !q.trim().is_empty() => q,
        _ => return "Error: 'query' argument required".to_string(),
    };
    let glob = args.get("glob").and_then(|v| v.as_str()).unwrap_or("*");
    let max_results = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(50);
    let py = format!(
        r#"import fnmatch, pathlib
root = pathlib.Path(".")
q = {q:?}
pat = {pat:?}
limit = {limit}
hits = 0
for p in root.rglob("*"):
    if not p.is_file():
        continue
    rel = p.as_posix()
    if not fnmatch.fnmatch(rel, pat):
        continue
    try:
        txt = p.read_text(errors="ignore")
    except Exception:
        continue
    for i, line in enumerate(txt.splitlines(), 1):
        if q in line:
            print(rel + ":" + str(i) + ":" + line[:240])
            hits += 1
            if hits >= limit:
                raise SystemExit(0)
"#,
        q = query,
        pat = glob,
        limit = max_results
    );
    let output = Command::new("python3")
        .arg("-c")
        .arg(py)
        .current_dir(project_root)
        .output()
        .await;
    match output {
        Ok(out) => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            let stderr = String::from_utf8_lossy(&out.stderr);
            if out.status.success() {
                if stdout.trim().is_empty() {
                    "No matches.".to_string()
                } else {
                    truncate_output(&stdout, 6000)
                }
            } else {
                format!("search_text failed: {}", truncate_output(&stderr, 1200))
            }
        }
        Err(e) => format!("search_text launch error: {}", e),
    }
}

fn resolve_existing_path(candidates: &[&str]) -> Option<PathBuf> {
    for p in candidates {
        if Path::new(p).exists() {
            return Some(PathBuf::from(*p));
        }
    }
    None
}

fn inject_satellite_env(cmd: &mut Command) {
    let mut vars: Vec<(String, String)> = Vec::new();
    for env_path in &[
        "/data/cesarops/repo/.env",
        "/mnt/data-external/cesarops/repo/.env",
        "/codebase/repos/wreckhunter2000-1/.env",
    ] {
        if let Ok(content) = fs::read_to_string(env_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let key = k.trim().to_string();
                    let val = v.trim().trim_matches('"').trim_matches('\'').to_string();
                    if key.starts_with("EARTHDATA")
                        || key.starts_with("NASA_EARTHDATA")
                        || key.starts_with("COPERNICUS")
                        || key.starts_with("USGS")
                        || key.starts_with("ASF")
                        || key == "CESAROPS_DATA_DIR"
                    {
                        vars.push((key, val));
                    }
                }
            }
        }
    }
    for (k, v) in vars {
        cmd.env(k, v);
    }
}

fn format_ymd(ts: i64) -> String {
    let secs_per_day = 86400i64;
    let days_since_epoch = ts / secs_per_day;
    let days = days_since_epoch + 719468;
    let era = if days >= 0 { days / 146097 } else { (days - 146096) / 146097 };
    let doe = days - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let yyyy = if m <= 2 { y + 1 } else { y };
    format!("{:04}-{:02}-{:02}", yyyy, m, d)
}

async fn scan_region(args: &Value) -> String {
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' required".to_string(),
    };
    let parts: Vec<&str> = bbox.split(',').map(|s| s.trim()).collect();
    if parts.len() != 4 {
        return "Error: 'bbox' must have 4 comma-separated floats".to_string();
    }
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as i64;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let start = format_ymd(now - (days * 86400));
    let end = format_ymd(now);
    let label = args.get("region_name").and_then(|v| v.as_str()).unwrap_or("mcp_scan");
    let out = format!("/tmp/scan_{}.json", now);
    let script = "/mnt/data-external/cesarops/repo/scan_cli.py";
    let mut cmd = Command::new("python3");
    cmd.arg(script)
        .arg("--bbox")
        .arg(parts[0]).arg(parts[1]).arg(parts[2]).arg(parts[3])
        .arg("--dates").arg(&start).arg(&end)
        .arg("--output").arg(&out)
        .arg("--label").arg(label)
        .arg("--download");
    inject_satellite_env(&mut cmd);
    match tokio::time::timeout(std::time::Duration::from_secs(1800), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => truncate_output(&String::from_utf8_lossy(&output.stdout), 2000),
        Ok(Ok(output)) => format!("scan_cli FAILED: {}", truncate_output(&String::from_utf8_lossy(&output.stderr), 1500)),
        Ok(Err(e)) => format!("Error launching scan_cli: {}", e),
        Err(_) => "Error: scan_region timed out".to_string(),
    }
}

async fn magnetic_dipole_detect(args: &Value) -> String {
    let grid = match args.get("grid_path").and_then(|v| v.as_str()) {
        Some(g) => g,
        None => return "Error: 'grid_path' required".to_string(),
    };
    let binary = std::env::var("AEROMAGNETIC_WORKER_BIN")
        .unwrap_or_else(|_| "/codebase/repos/wreckhunter2000-1/target/release/cesarops-aeromagnetic-worker".to_string());
    let mut cmd = Command::new(binary);
    cmd.arg("--grid").arg(grid)
        .arg("--pixel-size").arg(args.get("pixel_size_m").and_then(|v| v.as_f64()).unwrap_or(25.0).to_string())
        .arg("--inner").arg(args.get("inner_radius").and_then(|v| v.as_u64()).unwrap_or(10).to_string())
        .arg("--outer").arg(args.get("outer_radius").and_then(|v| v.as_u64()).unwrap_or(25).to_string())
        .arg("--min-score").arg(args.get("min_score").and_then(|v| v.as_f64()).unwrap_or(0.5).to_string())
        .arg("--top-n").arg(args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(100).to_string());
    match tokio::time::timeout(std::time::Duration::from_secs(300), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => truncate_output(&String::from_utf8_lossy(&output.stdout), 2000),
        Ok(Ok(output)) => format!("aeromagnetic FAILED: {}", truncate_output(&String::from_utf8_lossy(&output.stderr), 1500)),
        Ok(Err(e)) => format!("Error launching aeromagnetic worker: {}", e),
        Err(_) => "Error: magnetic_dipole_detect timed out".to_string(),
    }
}

async fn download_satellite_window(args: &Value) -> String {
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' required".to_string(),
    };
    let provider = args.get("provider").and_then(|v| v.as_str()).unwrap_or("auto");
    let sensors = match provider {
        "auto" | "all" => "all",
        "aws" | "optical_aws" | "stac" | "free_aws" => "aws",
        "sentinel" => "sentinel2_aws,sar",
        "sentinel2" => "sentinel2_aws",
        "landsat" => "landsat_aws",
        "ecostress" => "ecostress",
        "swot" => "swot",
        "optical" => "optical_aws",
        other => other,
    };
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as i64;
    let max_results = args.get("max_results").and_then(|v| v.as_u64()).unwrap_or(20);
    let output_dir = args.get("output_dir").and_then(|v| v.as_str()).unwrap_or("/tmp/cesarops_downloads/");
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let start = format_ymd(now - (days * 86400));
    let end = format_ymd(now);
    let script = resolve_existing_path(&[
        "/codebase/repos/wreckhunter2000-1/universal_downloader.py",
        "/mnt/data-external/cesarops/repo/universal_downloader.py",
    ])
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_else(|| "/codebase/repos/wreckhunter2000-1/universal_downloader.py".to_string());
    let mut cmd = Command::new("python3");
    cmd.arg(&script)
        .arg("--bbox").arg(bbox)
        .arg("--sensors").arg(sensors)
        .arg("--dates").arg(&start).arg(&end)
        .arg("--max-results").arg(max_results.to_string())
        .arg("--output").arg(output_dir);
    inject_satellite_env(&mut cmd);
    match tokio::time::timeout(std::time::Duration::from_secs(1800), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => truncate_output(&String::from_utf8_lossy(&output.stdout), 2500),
        Ok(Ok(output)) => format!("downloader FAILED: {}", truncate_output(&String::from_utf8_lossy(&output.stderr), 1500)),
        Ok(Err(e)) => format!("Error launching downloader: {}", e),
        Err(_) => "Error: download_satellite_window timed out".to_string(),
    }
}

async fn weather_window(args: &Value) -> String {
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' required".to_string(),
    };
    let bbox_parts: Vec<&str> = bbox.split(',').map(|s| s.trim()).collect();
    if bbox_parts.len() != 4 {
        return "Error: 'bbox' must be 4 comma-separated floats".to_string();
    }
    let lat_min: f64 = match bbox_parts[0].parse() { Ok(v) => v, Err(e) => return format!("Error parsing lat_min: {}", e) };
    let lon_min: f64 = match bbox_parts[1].parse() { Ok(v) => v, Err(e) => return format!("Error parsing lon_min: {}", e) };
    let lat_max: f64 = match bbox_parts[2].parse() { Ok(v) => v, Err(e) => return format!("Error parsing lat_max: {}", e) };
    let lon_max: f64 = match bbox_parts[3].parse() { Ok(v) => v, Err(e) => return format!("Error parsing lon_max: {}", e) };
    let center_lat = (lat_min + lat_max) / 2.0;
    let center_lon = (lon_min + lon_max) / 2.0;
    let check = args.get("check").and_then(|v| v.as_str()).unwrap_or("post_storm");
    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as i64;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let start = format_ymd(now - (days * 86400));
    let end = format_ymd(now);
    let py_script = format!(
        "import sys; sys.path.insert(0, '/codebase/repos/wreckhunter2000-1'); import json; from weather_service import get_scan_windows; w = get_scan_windows({lat}, {lon}, '{start}', '{end}'); w.pop('conditions', None); summary = {{k: len(v) if isinstance(v, list) else v for k, v in w.items()}}; out = {{'window_summary': summary, 'recommended_dates_post_storm': (w.get('post_storm_1', [])[-3:] + w.get('post_storm_2', [])[-3:]), 'recommended_dates_calm': w.get('calm', [])[-3:], 'check_filter': '{check}'}}; print(json.dumps(out, indent=2))",
        lat = center_lat, lon = center_lon, start = start, end = end, check = check
    );
    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(py_script);
    match tokio::time::timeout(std::time::Duration::from_secs(60), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => truncate_output(&String::from_utf8_lossy(&output.stdout), 2000),
        Ok(Ok(output)) => format!("weather_service FAILED: {}", truncate_output(&String::from_utf8_lossy(&output.stderr), 1500)),
        Ok(Err(e)) => format!("Error launching weather_service: {}", e),
        Err(_) => "Error: weather_window timed out".to_string(),
    }
}

fn detection_service_url() -> String {
    std::env::var("DETECTION_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "http://10.0.0.201:5580".to_string())
        .trim_end_matches('/')
        .to_string()
}

async fn detection_health(_args: &Value) -> String {
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };
    let base = detection_service_url();
    match client.get(format!("{}/health", base)).send().await {
        Ok(resp) => truncate_output(&resp.text().await.unwrap_or_default(), 1500),
        Err(e) => format!("Detection service OFFLINE at {}: {}", base, e),
    }
}

async fn detection_scan(args: &Value) -> String {
    let region = match args.get("region").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return "Error: 'region' required".to_string(),
    };
    let tiles = args.get("tiles").cloned().unwrap_or_else(|| serde_json::json!([]));
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(10)).build() {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };
    let body = serde_json::json!({ "region": region, "tiles": tiles });
    match client.post(format!("{}/scan", detection_service_url())).json(&body).send().await {
        Ok(resp) => truncate_output(&resp.text().await.unwrap_or_default(), 1500),
        Err(e) => format!("Error submitting scan: {}", e),
    }
}

async fn detection_poll(args: &Value) -> String {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return "Error: 'job_id' required".to_string(),
    };
    let client = match reqwest::Client::builder().timeout(std::time::Duration::from_secs(5)).build() {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };
    let url = format!("{}/scan/{}", detection_service_url(), job_id);
    match client.get(url).send().await {
        Ok(resp) => truncate_output(&resp.text().await.unwrap_or_default(), 2000),
        Err(e) => format!("Error polling job {}: {}", job_id, e),
    }
}

async fn sat_mission(args: &Value) -> String {
    let spec_path = match args.get("spec_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'spec_path' required".to_string(),
    };
    if !Path::new(spec_path).exists() {
        return format!("Error: spec file not found: {}", spec_path);
    }
    let py = resolve_existing_path(&[
        "/codebase/projects/pipelines/satellite/sat_mission_orchestrator.py",
        "/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py",
        "/mnt/data-external/projects/pipelines/satellite/sat_mission_orchestrator.py",
        "/mnt/t440/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py",
    ])
    .unwrap_or_else(|| PathBuf::from("/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py"));
    let cwd = py.parent().and_then(|p| p.parent()).map(|p| p.to_path_buf()).unwrap_or_else(|| PathBuf::from("/codebase/projects/pipelines"));
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);
    let mut cmd = Command::new("python3");
    cmd.arg(py).arg("--spec").arg(spec_path).current_dir(cwd);
    if dry_run {
        cmd.arg("--dry-run");
    }
    if let Some(knobs) = args.get("knobs") {
        cmd.arg("--knobs").arg(if knobs.is_string() { knobs.as_str().unwrap_or_default().to_string() } else { knobs.to_string() });
    }
    if let Some(stages) = args.get("stages").and_then(|v| v.as_array()) {
        let names: Vec<String> = stages.iter().filter_map(|s| s.as_str().map(|x| x.to_string())).collect();
        if !names.is_empty() {
            cmd.arg("--stages");
            for s in names {
                cmd.arg(s);
            }
        }
    }
    inject_satellite_env(&mut cmd);
    match tokio::time::timeout(std::time::Duration::from_secs(7200), cmd.output()).await {
        Ok(Ok(output)) if output.status.success() => truncate_output(&String::from_utf8_lossy(&output.stdout), 3000),
        Ok(Ok(output)) => format!("sat_mission FAILED: {}\n{}", truncate_output(&String::from_utf8_lossy(&output.stderr), 1200), truncate_output(&String::from_utf8_lossy(&output.stdout), 800)),
        Ok(Err(e)) => format!("Error launching sat_mission: {}", e),
        Err(_) => "Error: sat_mission timed out after 2 hours".to_string(),
    }
}

async fn sat_read_mission_report(args: &Value) -> String {
    let output_dir = if let Some(d) = args.get("output_dir").and_then(|v| v.as_str()) {
        d.to_string()
    } else if let Some(spec) = args.get("spec_path").and_then(|v| v.as_str()) {
        let content = match fs::read_to_string(spec) {
            Ok(c) => c,
            Err(e) => return format!("Error reading spec: {}", e),
        };
        let v: Value = match serde_json::from_str(&content) {
            Ok(v) => v,
            Err(e) => return format!("Error parsing spec JSON: {}", e),
        };
        match v.get("paths").and_then(|p| p.get("output_dir")).and_then(|o| o.as_str()) {
            Some(d) => d.to_string(),
            None => return "Error: spec has no paths.output_dir".to_string(),
        }
    } else {
        return "Error: 'output_dir' or 'spec_path' is required".to_string();
    };
    let which = args.get("which").and_then(|v| v.as_str()).unwrap_or("both");
    let mut out = serde_json::Map::new();
    let read_json = |name: &str| -> Option<Value> {
        let p = PathBuf::from(&output_dir).join(name);
        if !p.exists() {
            return None;
        }
        fs::read_to_string(&p).ok().and_then(|s| serde_json::from_str(&s).ok())
    };
    match which {
        "mission" => {
            if let Some(v) = read_json("mission_report.json") {
                out.insert("mission_report".to_string(), v);
            }
        }
        "validation" => {
            if let Some(v) = read_json("validation_report.json") {
                out.insert("validation_report".to_string(), v);
            }
        }
        _ => {
            if let Some(v) = read_json("mission_report.json") {
                out.insert("mission_report".to_string(), v);
            }
            if let Some(v) = read_json("validation_report.json") {
                out.insert("validation_report".to_string(), v);
            }
        }
    }
    if out.is_empty() {
        return format!("No reports found in {}", output_dir);
    }
    serde_json::to_string_pretty(&Value::Object(out))
        .map(|s| truncate_output(&s, 8000))
        .unwrap_or_else(|e| format!("Error serializing reports: {}", e))
}

/// Resolve a relative path against the project root.
fn resolve_path(relative: &str, project_root: &Path) -> PathBuf {
    let p = Path::new(relative);
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        project_root.join(p)
    }
}

/// Truncate output string to max length with ellipsis.
fn truncate_output(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}... [truncated, {} total bytes]", &s[..max], s.len())
    }
}

/// Get current timestamp in ISO format.
fn chrono_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "unknown".to_string())
}
