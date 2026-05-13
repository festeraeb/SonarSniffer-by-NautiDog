use axum::{routing::{get, post}, Router, Json, extract::Path};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::process::Command;
use tokio::time::sleep;
use tracing::{error, info, warn};

#[derive(Deserialize)]
struct Config {
    groups: HashMap<String, Vec<GroupItem>>,
}

#[derive(Deserialize)]
struct GroupItem {
    #[serde(rename = "type")]
    group_type: String,
    name: Option<String>,
    binary: Option<String>,
    args: Option<Vec<String>>,
    working_dir: Option<String>,
    health_check: Option<String>,
    health_url: Option<String>,
    restart_policy: Option<String>,
    commands: Option<Vec<String>>,
}

#[derive(Serialize)]
struct StatusResponse {
    processes: HashMap<String, ProcessState>,
}

#[derive(Serialize, Clone)]
struct ProcessState {
    status: String,
    restart_count: u32,
    last_restart_secs: u64,
}

struct ProcessHandle {
    child: Option<tokio::process::Child>,
    binary: String,
    args: Vec<String>,
    working_dir: String,
    health_check: String,
    health_url: Option<String>,
    restart_count: u32,
    last_restart: Instant,
    status: String,
}

static BACKOFFS: [u64; 4] = [5, 15, 60, 300];

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();
    info!("cesarops-watchdog starting");

    let config_str = std::fs::read_to_string("watchdog.toml").unwrap_or_default();
    let config: Config = toml::from_str(&config_str).unwrap_or_default();

    let state = std::sync::Arc::tokio::sync::RwLock::new(HashMap::new());

    // Sequential group startup
    for group in ["mounts", "network", "infra", "llm", "apps", "agent"] {
        if let Some(items) = config.groups.get(group) {
            for item in items {
                match item.group_type.as_deref() {
                    Some("shell") => {
                        if let Some(cmds) = &item.commands {
                            for cmd in cmds {
                                if let Err(e) = Command::new("sh")
                                    .arg("-c")
                                    .arg(cmd)
                                    .output()
                                    .await
                                {
                                    error!("Shell cmd failed: {}", e);
                                }
                            }
                        }
                    }
                    Some("process") => {
                        if let (Some(name), Some(binary)) = (&item.name, &item.binary) {
                            let args = item.args.clone().unwrap_or_default();
                            let dir = item.working_dir.clone().unwrap_or_default();
                            let hc = item.health_check.clone().unwrap_or_else(|| "pid".to_string());
                            let url = item.health_url.clone();

                            let mut cmd = Command::new(binary);
                            cmd.args(&args).current_dir(&dir);
                            
                            match cmd.spawn() {
                                Ok(child) => {
                                    info!("Spawned {} pid={}", name, child.id());
                                    let mut handles = state.write().await;
                                    handles.insert(name.clone(), ProcessHandle {
                                        child: Some(child),
                                        binary: binary.clone(),
                                        args,
                                        working_dir: dir,
                                        health_check: hc,
                                        health_url: url,
                                        restart_count: 0,
                                        last_restart: Instant::now(),
                                        status: "running".to_string(),
                                    });
                                }
                                Err(e) => error!("Failed to spawn {}: {}", name, e),
                            }
                        }
                    }
                    _ => warn!("Unknown group type: {}", item.group_type),
                }
            }
        }
    }

    // Health monitor loop
    let monitor_state = state.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(5)).await;
            let mut handles = monitor_state.write().await;
            let dead: Vec<String> = handles.iter()
                .filter(|(_, h)| h.child.is_none())
                .map(|(n, _)| n.clone())
                .collect();
            for name in dead { handles.remove(&name); }

            for (name, handle) in handles.iter_mut() {
                if let Some(child) = &handle.child {
                    match child.try_wait() {
                        Ok(Some(_)) => {
                            warn!("Process {} exited", name);
                            handle.child = None;
                            handle.status = "stopped".to_string();
                        }
                        Ok(None) => {
                            let healthy = check_health(handle).await;
                            handle.status = if healthy { "running".to_string() } else { "unhealthy".to_string() };
                        }
                        Err(e) => error!("Wait error for {}: {}", name, e),
                    }
                }
            }
        }
    });

    // Restart & backoff loop
    let restart_state = state.clone();
    tokio::spawn(async move {
        loop {
            sleep(Duration::from_secs(2)).await;
            let mut handles = restart_state.write().await;
            for (name, handle) in handles.iter_mut() {
                if handle.child.is_none() && handle.status != "stopped" {
                    let idx = (handle.restart_count % 4) as usize;
                    let wait = Duration::from_secs(BACKOFFS[idx]);
                    info!("Restarting {} after {}s", name, wait.as_secs());
                    sleep(wait).await;

                    let mut cmd = Command::new(&handle.binary);
                    cmd.args(&handle.args).current_dir(&handle.working_dir);
                    match cmd.spawn() {
                        Ok(child) => {
                            handle.child = Some(child);
                            handle.restart_count += 1;
                            handle.last_restart = Instant::now();
                            handle.status = "restarting".to_string();
                            info!("Restarted {} pid={}", name, child.id());
                        }
                        Err(e) => error!("Restart spawn failed {}: {}", name, e),
                    }
                }
            }
        }
    });

    // Heartbeat
    tokio::spawn(async move {
        let client = reqwest::Client::new();
        let mut interval = tokio::time::interval(Duration::from_secs(10));
        loop {
            interval.tick().await;
            if let Err(e) = client.post("http://localhost:9000/heartbeat")
                .json(&serde_json::json!({"status": "alive"}))
                .send().await
            {
                error!("Heartbeat failed: {}", e);
            }
        }
    });

    // HTTP endpoints
    let http_state = state.clone();
    let app = Router::new()
        .route("/status", get(move || {
            let s = http_state.clone();
            async move {
                let handles = s.read().await;
                let mut proc_states = HashMap::new();
                for (name, h) in handles.iter() {
                    proc_states.insert(name.clone(), ProcessState {
                        status: h.status.clone(),
                        restart_count: h.restart_count,
                        last_restart_secs: h.last_restart.elapsed().as_secs(),
                    });
                }
                Json(StatusResponse { processes: proc_states })
            }
        }))
        .route("/replace", post(|body: String| async move {
            if let Err(e) = self_replace::self_replace(&body) {
                error!("Replace failed: {}", e);
                return "failed";
            }
            "success"
        }))
        .route("/restart/{name}", post(|Path(name): Path<String>| async move {
            let mut handles = http_state.write().await;
            if let Some(h) = handles.get_mut(&name) {
                if let Some(child) = h.child.take() {
                    let _ = child.kill().await;
                }
                h.status = "restarting".to_string();
            }
            "ok"
        }));

    info!("HTTP server listening on 0.0.0.0:9001");
    if let Err(e) = axum::serve(
        tokio::net::TcpListener::bind("0.0.0.0:9001").await,
        app,
    ).await {
        error!("HTTP server error: {}", e);
    }
    Ok(())
}

async fn check_health(handle: &ProcessHandle) -> bool {
    match handle.health_check.as_str() {
        "url" => {
            if let Some(url) = &handle.health_url {
                if let Ok(resp) = reqwest::get(url).await {
                    return resp.status().is_success();
                }
            }
            false
        }
        "pid" => true,
        _ => true,
    }
}
