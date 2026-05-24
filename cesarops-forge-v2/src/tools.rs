use crate::AppState;
use crate::validator::{ValidatorConfig, run_validation, benchmark_tps, ping};
use serde_json::Value;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};
use tokio::process::Command;
use tracing::{info, warn};

/// Load satellite/earthdata credentials from .env, credentials.sh, or bootstrap scripts.
fn load_satellite_env() -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut ingest = |k: &str, v: &str| {
        if v.is_empty() || seen.contains(k) {
            return;
        }
        let satellite = k.starts_with("EARTHDATA")
            || k.starts_with("NASA_EARTHDATA")
            || k.starts_with("COPERNICUS")
            || k.starts_with("USGS")
            || k.starts_with("ASF")
            || k == "CESAROPS_DATA_DIR";
        if satellite {
            seen.insert(k.to_string());
            vars.push((k.to_string(), v.to_string()));
        }
    };

    for env_path in &[
        "/data/cesarops/repo/.env",
        "/mnt/data-external/cesarops/repo/.env",
        "/codebase/repos/wreckhunter2000-1/.env",
    ] {
        if let Ok(content) = std::fs::read_to_string(env_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    ingest(k.trim(), v.trim());
                }
            }
        }
    }

    for cred_path in &[
        "/codebase/repos/wreckhunter2000-1/scripts/credentials.sh",
        "/codebase/repos/wreckhunter2000-1/backup/wreckhunter2000-1/scripts/credentials.sh",
    ] {
        if let Ok(content) = std::fs::read_to_string(cred_path) {
            for line in content.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    let v = v.trim().trim_matches('"').trim_matches('\'');
                    ingest(k.trim(), v);
                }
            }
        }
    }

    // Bootstrap scripts (setup_h97_xeon.sh etc.) when .env is absent on T440
    for boot_path in &[
        "/codebase/repos/wreckhunter2000-1/backup/wreckhunter2000-1/scripts/setup_h97_xeon.sh",
        "/codebase/repos/wreckhunter2000-1/backup/wreckhunter2000-1/bootstrap_xenon.sh",
    ] {
        if let Ok(content) = std::fs::read_to_string(boot_path) {
            for line in content.lines() {
                let line = line.trim();
                if !line.starts_with("EARTHDATA_") || !line.contains('=') {
                    continue;
                }
                if let Some((k, v)) = line.split_once('=') {
                    ingest(k.trim(), v.trim());
                }
            }
        }
    }

    // ~/.ssh/credentials or credentials.ssh (user-provided cluster secrets file)
    if let Ok(home) = std::env::var("HOME") {
        for name in &["credentials", "credentials.ssh"] {
            let p = format!("{}/.ssh/{}", home, name);
            if let Ok(content) = std::fs::read_to_string(&p) {
                for line in content.lines() {
                    let line = line.trim();
                    if line.is_empty() || line.starts_with('#') || !line.contains('=') {
                        continue;
                    }
                    if let Some((k, v)) = line.split_once('=') {
                        ingest(k.trim(), v.trim());
                    }
                }
            }
        }
    }

    vars
}

/// Apply satellite credentials to a Command before spawning.
fn inject_satellite_env(cmd: &mut Command) {
    for (k, v) in load_satellite_env() {
        cmd.env(&k, &v);
    }
}

/// First existing path among candidates (T440 / NFS / data-external layouts).
fn resolve_existing_path(candidates: &[&str]) -> Option<PathBuf> {
    for p in candidates {
        if std::path::Path::new(p).exists() {
            return Some(PathBuf::from(*p));
        }
    }
    None
}

fn detection_service_url() -> String {
    if let Ok(url) = std::env::var("DETECTION_URL") {
        if !url.is_empty() {
            return url.trim_end_matches('/').to_string();
        }
    }
    // Prefer cesarops2 augment when set by resume script; else local.
    std::env::var("DETECTION_URL_DEFAULT")
        .unwrap_or_else(|_| "http://10.0.0.201:5580".to_string())
        .trim_end_matches('/')
        .to_string()
}

fn satellite_mission_orchestrator_py() -> PathBuf {
    resolve_existing_path(&[
        "/codebase/projects/pipelines/satellite/sat_mission_orchestrator.py",
        "/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py",
        "/mnt/data-external/projects/pipelines/satellite/sat_mission_orchestrator.py",
        "/mnt/t440/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py",
    ])
    .unwrap_or_else(|| {
        PathBuf::from(
            "/codebase/repos/wreckhunter2000-1/pipelines/satellite/sat_mission_orchestrator.py",
        )
    })
}

fn satellite_mission_cwd(py: &PathBuf) -> PathBuf {
    py.parent()
        .and_then(|p| p.parent())
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("/codebase/projects/pipelines"))
}

/// Counter for think_harder calls per session. Resets on /clear.
static THINK_HARDER_COUNT: AtomicU32 = AtomicU32::new(0);
const THINK_HARDER_LIMIT: u32 = u32::MAX; // No limit — let it search as much as it needs

/// Reset the think_harder counter (called on /clear).
pub fn reset_think_counter() {
    THINK_HARDER_COUNT.store(0, Ordering::Relaxed);
}

/// Execute a tool by name with the given arguments. Returns the tool result as a string.
pub async fn execute(name: &str, arguments: &Value, state: &AppState) -> String {
    info!("Tool call: {} args={}", name, arguments);

    if crate::mcp_delegate::should_delegate(name) {
        match crate::mcp_delegate::execute_on_mcp(name, arguments).await {
            Ok(r) => return format!("[mcp] {}", r),
            Err(e) => {
                warn!("MCP delegate for {} failed ({}), running locally", name, e);
            }
        }
    }

    match name {
        "write_file" => { reset_think_counter(); write_file(arguments, state).await },
        "read_file" => { reset_think_counter(); read_file(arguments, state).await },
        "cargo_check" => { reset_think_counter(); cargo_check(arguments, state).await },
        "think_harder" => {
            let _count = THINK_HARDER_COUNT.fetch_add(1, Ordering::Relaxed) + 1;
            let result = think_harder(arguments, state).await;
            result
        },
        "remember" => { reset_think_counter(); remember(arguments, state).await },
        "run_command" => { reset_think_counter(); run_command(arguments, state).await },
        "speed_check" => { speed_check(arguments, state).await },
        // ── Wreck-detection / SAR / downed-aircraft tools ────────────────
        // These shell out to proven Python (scan_engine, universal_downloader,
        // weather_service) and the Rust aeromagnetic worker. Same toolchain
        // serves wreck-hunting + search-and-rescue + downed-aircraft search.
        "scan_region" => { reset_think_counter(); scan_region(arguments, state).await },
        "magnetic_dipole_detect" => { reset_think_counter(); magnetic_dipole_detect(arguments, state).await },
        "download_satellite_window" => { reset_think_counter(); download_satellite_window(arguments, state).await },
        "weather_window" => { reset_think_counter(); weather_window(arguments, state).await },
        // ── Triple-lock detection HTTP service (cesarops-detection :5580) ─
        // Submit + poll detection jobs through the orchestration service.
        // Falls back gracefully if the service or TPU jitter VM is down.
        "detection_health" => { reset_think_counter(); detection_health(arguments, state).await },
        "detection_scan" => { reset_think_counter(); detection_scan(arguments, state).await },
        "detection_poll" => { reset_think_counter(); detection_poll(arguments, state).await },
        "sat_mission" => { reset_think_counter(); sat_mission(arguments, state).await },
        "sat_read_mission_report" => { reset_think_counter(); sat_read_mission_report(arguments, state).await },
        _ => format!(
            "Unknown tool: '{}'. Available: write_file, read_file, cargo_check, think_harder, remember, run_command, speed_check, scan_region, magnetic_dipole_detect, download_satellite_window, weather_window, detection_health, detection_scan, detection_poll, sat_mission, sat_read_mission_report",
            name
        ),
    }
}

/// Write content to a file under project_root.
async fn write_file(args: &Value, state: &AppState) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };

    let root = state.config.read().await.project_root.clone();
    let full_path = resolve_path(path, &root);

    // Create parent directories
    if let Some(parent) = full_path.parent() {
        if let Err(e) = tokio::fs::create_dir_all(parent).await {
            return format!("Error creating directories: {}", e);
        }
    }

    match tokio::fs::write(&full_path, content).await {
        Ok(_) => format!("Written {} bytes to {}", content.len(), path),
        Err(e) => format!("Error writing file: {}", e),
    }
}

/// Read a file, truncating at 3000 chars.
async fn read_file(args: &Value, state: &AppState) -> String {
    let path = match args.get("path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'path' argument required".to_string(),
    };

    let root = state.config.read().await.project_root.clone();
    let full_path = resolve_path(path, &root);

    match tokio::fs::read_to_string(&full_path).await {
        Ok(content) => {
            if content.len() > 3000 {
                format!("{}...\n[truncated at 3000 chars, total {} bytes]", &content[..3000], content.len())
            } else {
                content
            }
        }
        Err(e) => format!("Error reading file: {}", e),
    }
}

/// Run cargo check with --message-format=json, parse errors.
async fn cargo_check(args: &Value, state: &AppState) -> String {
    let dir = args
        .get("dir")
        .and_then(|v| v.as_str())
        .unwrap_or(".");

    let root = state.config.read().await.project_root.clone();
    let full_dir = resolve_path(dir, &root);

    let output = match Command::new("cargo")
        .args(["check", "--message-format=json"])
        .current_dir(&full_dir)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return format!("Error running cargo check: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // Extract compiler errors from JSON lines
    let mut errors: Vec<String> = Vec::new();
    for line in stdout.lines() {
        if let Ok(msg) = serde_json::from_str::<Value>(line) {
            if msg.get("reason").and_then(|r| r.as_str()) == Some("compiler-message") {
                if let Some(message) = msg.get("message") {
                    let level = message.get("level").and_then(|l| l.as_str()).unwrap_or("");
                    let text = message.get("message").and_then(|m| m.as_str()).unwrap_or("");
                    if level == "error" {
                        errors.push(text.to_string());
                    }
                }
            }
        }
    }

    if errors.is_empty() && output.status.success() {
        "cargo check: OK (no errors)".to_string()
    } else if errors.is_empty() {
        // Fallback to stderr
        let truncated = truncate_output(&stderr, 2000);
        format!("cargo check FAILED:\n{}", truncated)
    } else {
        let error_list = errors.iter().take(10).cloned().collect::<Vec<_>>().join("\n- ");
        format!("cargo check: {} error(s):\n- {}", errors.len(), error_list)
    }
}

/// Search nautivecs + WSO for information.
async fn think_harder(args: &Value, state: &AppState) -> String {
    let query = match args.get("query").and_then(|v| v.as_str()) {
        Some(q) => q,
        None => return "Error: 'query' argument required".to_string(),
    };

    let cfg = state.config.read().await;
    let client = reqwest::Client::new();
    let mut results = Vec::new();

    // Query nautivecs
    let nautivecs_result = client
        .post(&cfg.nautivecs_url)
        .json(&serde_json::json!({"query": query, "top_k": 3}))
        .send()
        .await;

    match nautivecs_result {
        Ok(resp) => {
            if let Ok(body) = resp.text().await {
                let truncated = truncate_output(&body, 1500);
                results.push(format!("[nautivecs]: {}", truncated));
            }
        }
        Err(e) => {
            warn!("nautivecs query failed: {}", e);
            results.push(format!("[nautivecs]: unavailable ({})", e));
        }
    }

    // Query WSO (web search)
    let wso_result = client
        .post(&cfg.wso_url)
        .json(&serde_json::json!({"query": query, "max_results": 3}))
        .send()
        .await;

    match wso_result {
        Ok(resp) => {
            if let Ok(body) = resp.text().await {
                let truncated = truncate_output(&body, 1500);
                results.push(format!("[web search]: {}", truncated));
            }
        }
        Err(e) => {
            warn!("WSO query failed: {}", e);
            results.push(format!("[web search]: unavailable ({})", e));
        }
    }

    if results.is_empty() {
        "No results from knowledge base or web search.".to_string()
    } else {
        results.join("\n\n")
    }
}

/// Append a lesson to research_log/lessons_learned.md AND push to nautivecs vector DB.
/// This makes the lesson retrievable via think_harder on future prompts.
async fn remember(args: &Value, state: &AppState) -> String {
    let content = match args.get("content").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'content' argument required".to_string(),
    };
    let tags = args
        .get("tags")
        .and_then(|v| v.as_str())
        .unwrap_or("general");

    let root = state.config.read().await.project_root.clone();
    let log_path = resolve_path("research_log/lessons_learned.md", &root);

    // Ensure directory exists
    if let Some(parent) = log_path.parent() {
        let _ = tokio::fs::create_dir_all(parent).await;
    }

    let entry = format!("\n## [{}] {}\n{}\n", tags, chrono_now(), content);

    // Write to markdown log
    let log_result = {
        let existing = tokio::fs::read_to_string(&log_path).await.unwrap_or_default();
        let new_content = format!("{}{}", existing, entry);
        tokio::fs::write(&log_path, new_content).await
    };

    // Push to nautivecs so think_harder can retrieve it
    // nautivecs /add endpoint: POST { "text": "...", "tags": "...", "source": "..." }
    let cfg = state.config.read().await;
    let nautivecs_base = cfg.nautivecs_url
        .trim_end_matches("/query")
        .trim_end_matches("/search");
    let add_url = format!("{}/add", nautivecs_base);

    let client = reqwest::Client::new();
    let nautivecs_result = client
        .post(&add_url)
        .json(&serde_json::json!({
            "text": format!("[{}] {}", tags, content),
            "tags": tags,
            "source": "lessons_learned",
            "file_path": "research_log/lessons_learned.md",
        }))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await;

    let nautivecs_status = match nautivecs_result {
        Ok(r) if r.status().is_success() => " + indexed in nautivecs".to_string(),
        Ok(r) => format!(" (nautivecs returned {})", r.status()),
        Err(e) => format!(" (nautivecs unavailable: {})", e),
    };

    match log_result {
        Ok(_) => format!(
            "Remembered (tags: {}){}: {}...",
            tags, nautivecs_status, &content[..content.len().min(80)]
        ),
        Err(e) => format!("Error writing memory: {}", e),
    }
}

/// Run a shell command with K-line guards.
async fn run_command(args: &Value, state: &AppState) -> String {
    let cmd = match args.get("cmd").and_then(|v| v.as_str()) {
        Some(c) => c,
        None => return "Error: 'cmd' argument required".to_string(),
    };

    // K-line guard: block dangerous commands
    let blocked = ["rm -rf /", "rm -rf /*", "dd if=", "mkfs", "> /dev/sd", "chmod 777 /"];
    for pattern in &blocked {
        if cmd.contains(pattern) {
            warn!("K-LINED command blocked: {}", cmd);
            return format!("BLOCKED: Command '{}' is K-lined (dangerous operation)", cmd);
        }
    }

    let root = state.config.read().await.project_root.clone();
    let output = match Command::new("bash")
        .args(["-c", cmd])
        .current_dir(&root)
        .output()
        .await
    {
        Ok(o) => o,
        Err(e) => return format!("Error executing command: {}", e),
    };

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    let mut result = String::new();
    if !stdout.is_empty() {
        result.push_str(&truncate_output(&stdout, 2000));
    }
    if !stderr.is_empty() {
        if !result.is_empty() {
            result.push_str("\n[stderr]: ");
        }
        result.push_str(&truncate_output(&stderr, 500));
    }

    if result.is_empty() {
        format!("Command completed (exit code: {})", output.status.code().unwrap_or(-1))
    } else {
        result
    }
}

/// Ad-hoc speed + accuracy check using the P1000 TinyLlama reference.
///
/// Fires the same prompt at both the main engine and the P1000 in parallel,
/// compares token agreement, and reports t/s for both.
///
/// Usage: speed_check { "prompt": "optional — defaults to a fixed benchmark prompt" }
async fn speed_check(args: &Value, state: &AppState) -> String {
    let prompt = args
        .get("prompt")
        .and_then(|v| v.as_str())
        .unwrap_or("The quick brown fox jumps over the lazy dog. In Rust, a vector is");

    let cfg = state.config.read().await;
    let validator_url = cfg.validator_url.clone();

    // First check if P1000 is up
    if !ping(&validator_url).await {
        // P1000 offline — just benchmark the main engine
        info!("P1000 validator offline, benchmarking main engine only");
        let main_tps = benchmark_tps(&cfg.coder_url, 10).await;
        return match main_tps {
            Some(tps) => format!(
                "⚡ Speed check (P1000 offline — main engine only)\n\
                 Main engine: {:.1} t/s\n\
                 P1000 ({}): offline",
                tps, validator_url
            ),
            None => format!(
                "Speed check failed — main engine ({}) also unreachable",
                cfg.coder_url
            ),
        };
    }

    let config = ValidatorConfig {
        main_endpoint: cfg.coder_url.clone(),
        ref_endpoint: validator_url.clone(),
        n_tokens: 10,
        min_agreement: 0.4, // TinyLlama vs 35B will diverge — 40% is fine
    };

    let result = run_validation(&config, prompt).await;

    format!(
        "⚡ Speed + Accuracy Check\n\
         Prompt: \"{}\"\n\
         {}\n\
         Main tokens:  {}\n\
         P1000 tokens: {}\n\
         \n\
         Note: Token agreement between different model sizes is expected to be ~40-70%.\n\
         Low agreement (<30%) on simple prompts may indicate main engine issues.",
        &prompt[..prompt.len().min(60)],
        result.summary,
        result.main_tokens.join(" "),
        result.ref_tokens.join(" "),
    )
}

// --- Helpers ---

fn resolve_path(relative: &str, root: &str) -> PathBuf {
    PathBuf::from(root).join(relative)
}

fn truncate_output(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...[truncated, {} total bytes]", &s[..max], s.len())
    } else {
        s.to_string()
    }
}

/// Simple timestamp without pulling in chrono crate.
fn chrono_now() -> String {
    // Use system time as unix timestamp
    let duration = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", duration.as_secs())
}


// ────────────────────────────────────────────────────────────────────────────
// Wreck detection / SAR / downed-aircraft tools
// ────────────────────────────────────────────────────────────────────────────
//
// Same toolchain serves three missions:
//   1. Wreck hunting   — detect sunken vessels in 500ft of water from satellite
//   2. Search & rescue — locate missing vessels / aircraft in same waters
//   3. Downed aircraft — magnetic + glint paint + thermal signatures
//
// The physics doesn't change between schooner and Cessna — only the
// validation database (known_wrecks.json vs known_aircraft.json) and
// the spectral priors. That's why these are generic.

/// Run the proven Python scan_cli (scan_engine library is invoked through it).
/// Engine implements 7-pass detection: anomaly + hydrocarbon + Stumpf
/// bathymetry + LoG + SWIR silt erasure + mussel clearspot + triple-lock.
async fn scan_region(args: &Value, _state: &AppState) -> String {
    // bbox arrives as "lat_min,lon_min,lat_max,lon_max" string. scan_cli
    // takes 4 separate floats via nargs=4 argument so we split.
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' (lat_min,lon_min,lat_max,lon_max) is required".to_string(),
    };
    let bbox_parts: Vec<&str> = bbox.split(',').map(|s| s.trim()).collect();
    if bbox_parts.len() != 4 {
        return format!("Error: 'bbox' must be 4 comma-separated floats, got: {}", bbox);
    }

    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as i64;
    // mode is a forge-side hint for the operator log; scan_cli doesn't use it
    let _mode = args.get("mode").and_then(|v| v.as_str()).unwrap_or("wreck");
    let label = args.get("region_name").and_then(|v| v.as_str()).unwrap_or("forge_scan");

    // scan_cli requires --dates START END (YYYY-MM-DD), build from days arg
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let end_ts = now;
    let start_ts = now - (days * 86400);
    let format_date = |ts: i64| -> String {
        // YYYY-MM-DD via libc-free arithmetic
        let secs_per_day = 86400i64;
        let days_since_epoch = ts / secs_per_day;
        // Days from 1970-01-01 to {y, m, d}; algorithm from RFC 3339 / civil_from_days
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
    };
    let start_date = format_date(start_ts);
    let end_date = format_date(end_ts);

    let timestamp = now;
    let output_path = format!("/tmp/scan_{}.json", timestamp);
    let script_path = "/mnt/data-external/cesarops/repo/scan_cli.py";

    info!("scan_region: bbox=[{}] dates={}..{} label={} output={}",
        bbox_parts.join(","), start_date, end_date, label, output_path);

    let mut cmd = Command::new("python3");
    cmd.arg(script_path)
        .arg("--bbox")
        .arg(bbox_parts[0]).arg(bbox_parts[1])
        .arg(bbox_parts[2]).arg(bbox_parts[3])
        .arg("--dates").arg(&start_date).arg(&end_date)
        .arg("--output").arg(&output_path)
        .arg("--label").arg(label)
        .arg("--download");  // auto-download the satellite tiles
    inject_satellite_env(&mut cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1800),
        cmd.output(),
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            // scan_cli writes results to the output dir. Read summary from stdout.
            let stdout = String::from_utf8_lossy(&output.stdout);
            truncate_output(stdout.trim(), 2000)
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("scan_cli FAILED (exit {}): {}",
                output.status.code().unwrap_or(-1),
                truncate_output(&stderr, 1500))
        }
        Ok(Err(e)) => format!("Error launching python {}: {}", script_path, e),
        Err(_) => "Error: scan_cli timed out after 30 minutes".to_string(),
    }
}

/// Magnetic dipole detection via the cesarops-aeromagnetic-worker Rust binary.
/// Real wgpu compute shader running on Pascal — validated against known
/// wreck sites in Lake Erie + Straits of Mackinac.
async fn magnetic_dipole_detect(args: &Value, _state: &AppState) -> String {
    let grid_path = match args.get("grid_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'grid_path' (CSV or NPY magnetic grid) is required".to_string(),
    };
    let pixel_size = args.get("pixel_size_m").and_then(|v| v.as_f64()).unwrap_or(25.0) as f32;
    let inner = args.get("inner_radius").and_then(|v| v.as_u64()).unwrap_or(10) as u32;
    let outer = args.get("outer_radius").and_then(|v| v.as_u64()).unwrap_or(25) as u32;
    let min_score = args.get("min_score").and_then(|v| v.as_f64()).unwrap_or(0.5) as f32;
    let top_n = args.get("top_n").and_then(|v| v.as_u64()).unwrap_or(100) as u32;

    let binary_path = crate::paths::aeromagnetic_worker_binary();

    info!("magnetic_dipole_detect: grid={} pixel_size={} inner={} outer={}",
        grid_path, pixel_size, inner, outer);

    if !std::path::Path::new(&binary_path).exists() {
        return format!(
            "Error: cesarops-aeromagnetic-worker binary not found at {}. \
             Build it with: cargo build --release -p cesarops-aeromagnetic-worker",
            binary_path
        );
    }

    let mut cmd = Command::new(&binary_path);
    cmd.arg("--grid").arg(grid_path)
        .arg("--pixel-size").arg(pixel_size.to_string())
        .arg("--inner").arg(inner.to_string())
        .arg("--outer").arg(outer.to_string())
        .arg("--min-score").arg(min_score.to_string())
        .arg("--top-n").arg(top_n.to_string());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(300),
        cmd.output(),
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            truncate_output(&String::from_utf8_lossy(&output.stdout), 2000)
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("aeromagnetic_worker FAILED (exit {}): {}",
                output.status.code().unwrap_or(-1),
                truncate_output(&stderr, 1500))
        }
        Ok(Err(e)) => format!("Error launching {}: {}", binary_path, e),
        Err(_) => "Error: magnetic_dipole_detect timed out after 5 minutes".to_string(),
    }
}

/// Download a satellite tile window for the given bbox via the unified
/// 5-source downloader (ASF, Copernicus, PO.DAAC, USGS, HLS).
async fn download_satellite_window(args: &Value, _state: &AppState) -> String {
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' (lat_min,lon_min,lat_max,lon_max) is required".to_string(),
    };
    // Map our friendly 'provider' to universal_downloader's --sensors flag
    let provider = args.get("provider").and_then(|v| v.as_str()).unwrap_or("auto");
    let sensors = match provider {
        "auto" | "all" => "all",
        // Element84 STAC + public S3 COGs — no Earthdata auth (optical, SWIR, Landsat thermal)
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
    let max_results = args
        .get("max_results")
        .and_then(|v| v.as_u64())
        .unwrap_or(20) as u32;
    let output_dir = args
        .get("output_dir")
        .and_then(|v| v.as_str())
        .unwrap_or("/tmp/cesarops_downloads/");

    // Build YYYY-MM-DD date pair from days window
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let format_date = |ts: i64| -> String {
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
    };
    let end_date = format_date(now);
    let start_date = format_date(now - (days * 86400));

    let script_path = resolve_existing_path(&[
        "/codebase/repos/wreckhunter2000-1/universal_downloader.py",
        "/mnt/data-external/cesarops/repo/universal_downloader.py",
    ])
    .map(|p| p.to_string_lossy().to_string())
    .unwrap_or_else(|| "/codebase/repos/wreckhunter2000-1/universal_downloader.py".to_string());

    info!(
        "download_satellite_window: bbox={} sensors={} dates={}..{} -> {}",
        bbox, sensors, start_date, end_date, output_dir
    );

    let mut cmd = Command::new("python3");
    cmd.arg(&script_path)
        .arg("--bbox").arg(bbox)
        .arg("--sensors").arg(sensors)
        .arg("--dates").arg(&start_date).arg(&end_date)
        .arg("--max-results").arg(max_results.to_string())
        .arg("--output").arg(output_dir);
    inject_satellite_env(&mut cmd);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(1800),
        cmd.output(),
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            truncate_output(stdout.trim(), 2000)
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("downloader FAILED (exit {}): {}",
                output.status.code().unwrap_or(-1),
                truncate_output(&stderr, 1500))
        }
        Ok(Err(e)) => format!("Error launching python {}: {}", script_path, e),
        Err(_) => "Error: download_satellite_window timed out after 30 minutes".to_string(),
    }
}

/// Weather window classification — wreck/SAR scans benefit from
/// post-storm windows (sediment settled, water clear, features
/// stirred up) over calm or storm conditions.
///
/// weather_service.py is a LIBRARY, not a CLI. We invoke get_scan_windows()
/// via a one-liner python -c. Center of bbox becomes the lat/lon for
/// weather lookup.
async fn weather_window(args: &Value, _state: &AppState) -> String {
    let bbox = match args.get("bbox").and_then(|v| v.as_str()) {
        Some(b) => b,
        None => return "Error: 'bbox' (lat_min,lon_min,lat_max,lon_max) is required".to_string(),
    };
    let bbox_parts: Vec<&str> = bbox.split(',').map(|s| s.trim()).collect();
    if bbox_parts.len() != 4 {
        return format!("Error: 'bbox' must be 4 comma-separated floats, got: {}", bbox);
    }
    let lat_min: f64 = match bbox_parts[0].parse() {
        Ok(v) => v, Err(e) => return format!("Error parsing lat_min: {}", e),
    };
    let lon_min: f64 = match bbox_parts[1].parse() {
        Ok(v) => v, Err(e) => return format!("Error parsing lon_min: {}", e),
    };
    let lat_max: f64 = match bbox_parts[2].parse() {
        Ok(v) => v, Err(e) => return format!("Error parsing lat_max: {}", e),
    };
    let lon_max: f64 = match bbox_parts[3].parse() {
        Ok(v) => v, Err(e) => return format!("Error parsing lon_max: {}", e),
    };
    let center_lat = (lat_min + lat_max) / 2.0;
    let center_lon = (lon_min + lon_max) / 2.0;

    let check = args.get("check").and_then(|v| v.as_str())
        .or_else(|| args.get("condition").and_then(|v| v.as_str()))  // panel sends 'condition'
        .unwrap_or("post_storm");

    let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14) as i64;

    // Build YYYY-MM-DD start/end window ending today
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let format_date = |ts: i64| -> String {
        let secs_per_day = 86400i64;
        let days_since_epoch = ts / secs_per_day;
        let dd = days_since_epoch + 719468;
        let era = if dd >= 0 { dd / 146097 } else { (dd - 146096) / 146097 };
        let doe = dd - era * 146097;
        let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let yyyy = if m <= 2 { y + 1 } else { y };
        format!("{:04}-{:02}-{:02}", yyyy, m, d)
    };
    let end_date = format_date(now);
    let start_date = format_date(now - (days * 86400));

    info!("weather_window: lat={:.4} lon={:.4} dates={}..{} check={}",
        center_lat, center_lon, start_date, end_date, check);

    // weather_service.py is a library — invoke via python -c importing it.
    // Output is JSON to stdout.
    let repo_root = "/codebase/repos/wreckhunter2000-1";
    let py_script = format!(
        "import sys; sys.path.insert(0, '{repo}'); sys.path.insert(0, '/mnt/data-external/cesarops/repo'); \
         import json; \
         from weather_service import get_scan_windows; \
         w = get_scan_windows({lat}, {lon}, '{start}', '{end}'); \
         w.pop('conditions', None); \
         summary = {{k: len(v) if isinstance(v, list) else v for k, v in w.items()}}; \
         out = {{'window_summary': summary, \
                'recommended_dates_post_storm': (w.get('post_storm_1', [])[-3:] + w.get('post_storm_2', [])[-3:]), \
                'recommended_dates_calm': w.get('calm', [])[-3:], \
                'check_filter': '{check}'}}; \
         print(json.dumps(out, indent=2))",
        repo = repo_root,
        lat = center_lat,
        lon = center_lon,
        start = start_date,
        end = end_date,
        check = check,
    );

    let mut cmd = Command::new("python3");
    cmd.arg("-c").arg(&py_script);

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        cmd.output(),
    ).await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            truncate_output(stdout.trim(), 2000)
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            format!("weather_service FAILED: {}",
                truncate_output(&stderr, 1500))
        }
        Ok(Err(e)) => format!("Error launching python: {}", e),
        Err(_) => "Error: weather_window timed out after 60 seconds".to_string(),
    }
}


// ────────────────────────────────────────────────────────────────────────────
// Triple-lock detection service (cesarops-detection on port 5580)
// ────────────────────────────────────────────────────────────────────────────
//
// The detection service orchestrates the Triple-Lock pipeline:
//   Scout (1060 Florence-2)  → Validator (P1000 Moondream2)  → Jitter (TPU VM)
//
// If the TPU VM is offline, the pipeline runs in 2-lock degraded mode
// (Confirmed promotes to Investigate on jitter unavailability).
//
// Service must be started first via:
//   cesarops-detection/scripts/start.sh
// or the worker control panel in the forge.

/// Probe detection service health + report worker status.
async fn detection_health(_args: &Value, _state: &AppState) -> String {
    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };

    let base = detection_service_url();
    match client.get(format!("{}/health", base)).send().await {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(json) => {
                    let workers = json.get("workers");
                    let scout_ok = workers
                        .and_then(|w| w.get("scout_1060"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let val_ok = workers
                        .and_then(|w| w.get("validator_p1000"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);
                    let jitter_ok = workers
                        .and_then(|w| w.get("jitter_tpu"))
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                    let status = |b: bool| if b { "OK" } else { "OFFLINE" };
                    let degrade = if !jitter_ok {
                        "  (pipeline runs in 2-lock degraded mode — Confirmed -> Investigate on jitter offline)"
                    } else {
                        ""
                    };

                    format!(
                        "Detection service: ONLINE\n\
                         Scout (1060):     {}\n\
                         Validator (P1000): {}\n\
                         Jitter (TPU):     {}\n\
                         {}",
                        status(scout_ok),
                        status(val_ok),
                        status(jitter_ok),
                        degrade
                    )
                }
                Err(e) => format!("Detection service responded but JSON malformed: {}", e),
            }
        }
        Err(_) => {
            format!(
                "Detection service OFFLINE on {}.\n\
             Start cesarops2 lab (:5580) or T440: cesarops-detection/scripts/start.sh\n\
             Set DETECTION_URL=http://10.0.0.201:5580 for augment.",
                base
            )
                .to_string()
        }
    }
}

/// Submit a tile-list scan job to the detection service.
/// Returns the job_id for subsequent polling.
async fn detection_scan(args: &Value, _state: &AppState) -> String {
    let region = match args.get("region").and_then(|v| v.as_str()) {
        Some(r) => r,
        None => return "Error: 'region' (string label, e.g. \"lake_erie_central\") is required".to_string(),
    };
    let tiles = match args.get("tiles") {
        Some(t) if t.is_array() => t.clone(),
        None => serde_json::json!([]),
        _ => return "Error: 'tiles' must be an array of {lat, lon, image_b64} objects".to_string(),
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };

    let body = serde_json::json!({
        "region": region,
        "tiles": tiles,
    });

    info!("detection_scan: region={} tiles={}", region,
        tiles.as_array().map(|a| a.len()).unwrap_or(0));

    let base = detection_service_url();
    match client.post(format!("{}/scan", base)).json(&body).send().await {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(json) => {
                    let job_id = json
                        .get("job_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown");
                    let tile_count = json
                        .get("tiles")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    format!(
                        "Scan submitted. job_id={} ({} tiles queued). \
                         Use detection_poll {{ \"job_id\": \"{}\" }} to check progress.",
                        job_id, tile_count, job_id
                    )
                }
                Err(e) => format!("Detection service replied but JSON malformed: {}", e),
            }
        }
        Err(e) => format!(
            "Error submitting scan: {}. Is detection service running at {}?",
            e,
            detection_service_url()
        ),
    }
}

/// Poll a previously-submitted scan job for status + confirmed detections.
async fn detection_poll(args: &Value, _state: &AppState) -> String {
    let job_id = match args.get("job_id").and_then(|v| v.as_str()) {
        Some(id) => id,
        None => return "Error: 'job_id' (uuid string from detection_scan) is required".to_string(),
    };

    let client = match reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(e) => return format!("Error building HTTP client: {}", e),
    };

    let url = format!("{}/scan/{}", detection_service_url(), job_id);
    match client.get(&url).send().await {
        Ok(resp) => {
            match resp.json::<Value>().await {
                Ok(json) => {
                    // Pretty-print but truncate long detection arrays
                    let pretty = serde_json::to_string_pretty(&json)
                        .unwrap_or_else(|_| json.to_string());
                    truncate_output(&pretty, 2000)
                }
                Err(e) => format!("Poll response not JSON: {}", e),
            }
        }
        Err(e) => format!("Error polling job {}: {}", job_id, e),
    }
}

/// Run the JSON satellite mission orchestrator (Python stages + knobs).
async fn sat_mission(args: &Value, _state: &AppState) -> String {
    let spec_path = match args.get("spec_path").and_then(|v| v.as_str()) {
        Some(p) => p,
        None => return "Error: 'spec_path' (mission JSON) is required".to_string(),
    };
    if !std::path::Path::new(spec_path).exists() {
        return format!("Error: spec file not found: {}", spec_path);
    }

    let py = satellite_mission_orchestrator_py();
    if !py.exists() {
        return format!("Error: sat_mission_orchestrator.py not found (tried {})", py.display());
    }

    let cwd = satellite_mission_cwd(&py);
    let dry_run = args.get("dry_run").and_then(|v| v.as_bool()).unwrap_or(false);

    let mut cmd = Command::new("python3");
    cmd.arg(&py).arg("--spec").arg(spec_path);
    if dry_run {
        cmd.arg("--dry-run");
    }
    if let Some(knobs) = args.get("knobs") {
        let knobs_str = if knobs.is_string() {
            knobs.as_str().unwrap().to_string()
        } else {
            knobs.to_string()
        };
        cmd.arg("--knobs").arg(knobs_str);
    }
    if let Some(stages) = args.get("stages").and_then(|v| v.as_array()) {
        let names: Vec<String> = stages
            .iter()
            .filter_map(|s| s.as_str().map(|x| x.to_string()))
            .collect();
        if !names.is_empty() {
            cmd.arg("--stages");
            for s in &names {
                cmd.arg(s);
            }
        }
    }
    cmd.current_dir(&cwd);
    inject_satellite_env(&mut cmd);

    info!("sat_mission: spec={} dry_run={} cwd={}", spec_path, dry_run, cwd.display());

    let result = tokio::time::timeout(
        std::time::Duration::from_secs(7200),
        cmd.output(),
    )
    .await;

    match result {
        Ok(Ok(output)) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let mut msg = truncate_output(stdout.trim(), 3000);
            if let Ok(spec) = std::fs::read_to_string(spec_path) {
                if let Ok(v) = serde_json::from_str::<Value>(&spec) {
                    if let Some(out) = v
                        .get("paths")
                        .and_then(|p| p.get("output_dir"))
                        .and_then(|o| o.as_str())
                    {
                        msg.push_str(&format!("\noutput_dir={}", out));
                    } else if let Some(mid) = v.get("mission_id").and_then(|m| m.as_str()) {
                        let guess = format!(
                            "/mnt/data-external/projects/pipelines/detection_runs/satellite/{}",
                            mid.to_lowercase().replace('_', "_")
                        );
                        msg.push_str(&format!("\noutput_dir_guess={}", guess));
                    }
                }
            }
            msg
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let stdout = String::from_utf8_lossy(&output.stdout);
            format!(
                "sat_mission FAILED (exit {}):\n{}\n{}",
                output.status.code().unwrap_or(-1),
                truncate_output(stderr.trim(), 1200),
                truncate_output(stdout.trim(), 800)
            )
        }
        Ok(Err(e)) => format!("Error launching sat_mission: {}", e),
        Err(_) => "Error: sat_mission timed out after 2 hours".to_string(),
    }
}

/// Read mission_report.json and/or validation_report.json from a mission output dir.
async fn sat_read_mission_report(args: &Value, _state: &AppState) -> String {
    let output_dir = if let Some(d) = args.get("output_dir").and_then(|v| v.as_str()) {
        d.to_string()
    } else if let Some(spec) = args.get("spec_path").and_then(|v| v.as_str()) {
        match resolve_existing_path(&[spec]) {
            Some(_) => {
                let content = match std::fs::read_to_string(spec) {
                    Ok(c) => c,
                    Err(e) => return format!("Error reading spec: {}", e),
                };
                let v: Value = match serde_json::from_str(&content) {
                    Ok(v) => v,
                    Err(e) => return format!("Error parsing spec JSON: {}", e),
                };
                match v
                    .get("paths")
                    .and_then(|p| p.get("output_dir"))
                    .and_then(|o| o.as_str())
                {
                    Some(d) => d.to_string(),
                    None => return "Error: spec has no paths.output_dir".to_string(),
                }
            }
            None => return format!("Error: spec_path not found: {}", spec),
        }
    } else {
        return "Error: 'output_dir' or 'spec_path' is required".to_string();
    };
    let which = args
        .get("which")
        .and_then(|v| v.as_str())
        .unwrap_or("both");

    let mut out = serde_json::Map::new();
    let read_json = |name: &str| -> Option<Value> {
        let p = PathBuf::from(&output_dir).join(name);
        if !p.exists() {
            return None;
        }
        std::fs::read_to_string(&p)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
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
        return format!(
            "No reports found in {} (expected mission_report.json / validation_report.json)",
            &output_dir
        );
    }

    serde_json::to_string_pretty(&Value::Object(out))
        .map(|s| truncate_output(&s, 8000))
        .unwrap_or_else(|e| format!("Error serializing reports: {}", e))
}

