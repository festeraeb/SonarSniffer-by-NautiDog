use serde::Serialize;
use std::collections::HashMap;
use tokio::process::Command;

/// GPU metrics from nvidia-smi (+ optional process list like nvtop).
#[derive(Debug, Clone, Serialize)]
pub struct GpuMetrics {
    pub gpus: Vec<GpuInfo>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuProcess {
    pub pid: u32,
    pub name: String,
    pub sm_pct: u32,
    pub mem_pct: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GpuInfo {
    pub index: u32,
    /// NVML GPU UUID — stable identity across reboots (not MAC; unique per physical card).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uuid: Option<String>,
    /// PCIe bus id from nvidia-smi, e.g. `00000000:0D:00.0`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pci_bus_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub serial: Option<String>,
    pub name: String,
    pub temperature_c: u32,
    pub utilization_pct: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub memory_util_pct: Option<u32>,
    pub memory_used_mb: u32,
    pub memory_total_mb: u32,
    pub power_draw_w: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fan_speed_pct: Option<u32>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub processes: Vec<GpuProcess>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    /// Discovered HTTP port for llama-server/kobold on this GPU (from process + socket scan).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmdline_model: Option<String>,
}

/// Query GPU metrics via nvidia-smi CSV (NVML — same source as nvtop).
pub async fn query_gpu_metrics() -> GpuMetrics {
    let hn = std::fs::read_to_string("/etc/hostname").unwrap_or_default().to_lowercase();
    let label = if hn.contains("t440") {
        "t440cesarops"
    } else if hn.contains("cesarops2") {
        "cesarops2"
    } else {
        hn.split('.').next().unwrap_or("local")
    };
    query_gpu_metrics_on_host(label, None).await
}

/// Normalize PCI bus id for matching config shorthand (`5E:00.0`) to NVML (`00000000:5E:00.0`).
pub fn pci_bus_ids_match(config_bus: &str, live_bus: &str) -> bool {
    let c = config_bus.trim().to_uppercase();
    let l = live_bus.trim().to_uppercase();
    if c.is_empty() || l.is_empty() {
        return false;
    }
    c == l || l.ends_with(&c) || l.contains(&format!(":{}", c.trim_start_matches("00000000:")))
}

pub fn gpu_identity_key(node: &str, uuid: &str) -> String {
    format!("{}:{}", node, uuid)
}

async fn run_shell(ssh_target: Option<&str>, script: &str) -> Option<String> {
    let output = if let Some(target) = ssh_target {
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=5",
                target,
                "bash",
                "-lc",
                script,
            ])
            .output()
            .await
            .ok()?
    } else {
        Command::new("bash").args(["-lc", script]).output().await.ok()?
    };
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

fn parse_mib(s: &str) -> u64 {
    let t = s.trim();
    if let Some(n) = t.strip_suffix(" MiB") {
        n.trim().parse().unwrap_or(0)
    } else if let Some(n) = t.strip_suffix(" GiB") {
        n.trim().parse::<u64>().unwrap_or(0) * 1024
    } else {
        t.parse().unwrap_or(0)
    }
}

fn parse_cmdline_port_model(args: &str) -> (Option<u16>, Option<String>) {
    let port = args
        .split_whitespace()
        .enumerate()
        .find_map(|(i, tok)| {
            if tok == "--port" {
                args.split_whitespace().nth(i + 1)?.parse().ok()
            } else if let Some(rest) = tok.strip_prefix("--port=") {
                rest.parse().ok()
            } else {
                None
            }
        });
    let model = args
        .split_whitespace()
        .enumerate()
        .find_map(|(i, tok)| {
            if tok == "-m" {
                args.split_whitespace().nth(i + 1).map(|s| s.to_string())
            } else {
                None
            }
        });
    (port, model)
}

/// Map GPU UUID → discovered llama/kobold listen port (+ model path from cmdline).
/// Ports float; identity is NVML UUID. Picks primary GPU per PID via max `used_gpu_memory`.
pub async fn discover_gpu_listen_ports_by_uuid(
    ssh_target: Option<&str>,
) -> HashMap<String, (u16, Option<String>)> {
    let mut out: HashMap<String, (u16, Option<String>)> = HashMap::new();

    // pid → (uuid, mem_mib) — keep row with highest memory per pid (multi-GPU visibility)
    let mut pid_primary_uuid: HashMap<u32, (String, u64)> = HashMap::new();
    let apps_script = r#"nvidia-smi --query-compute-apps=gpu_uuid,pid,used_gpu_memory --format=csv,noheader,nounits 2>/dev/null || true"#;
    if let Some(stdout) = run_shell(ssh_target, apps_script).await {
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if parts.len() < 3 {
                continue;
            }
            let uuid = parts[0].to_string();
            let Ok(pid) = parts[1].parse::<u32>() else {
                continue;
            };
            let mem = parse_mib(parts[2]);
            let entry = pid_primary_uuid
                .entry(pid)
                .or_insert((uuid.clone(), mem));
            if mem > entry.1 {
                *entry = (uuid, mem);
            }
        }
    }

    let mut pid_port: HashMap<u32, u16> = HashMap::new();
    let ss_script = r#"ss -tlnp 2>/dev/null | grep -E 'llama-server|koboldcpp' || true"#;
    if let Some(stdout) = run_shell(ssh_target, ss_script).await {
        for line in stdout.lines() {
            let port = line
                .split(':')
                .nth(1)
                .and_then(|s| s.split_whitespace().next())
                .and_then(|s| s.parse::<u16>().ok());
            let pid = line
                .split("pid=")
                .nth(1)
                .and_then(|s| s.split(',').next())
                .and_then(|s| s.parse::<u32>().ok());
            if let (Some(port), Some(pid)) = (port, pid) {
                pid_port.insert(pid, port);
            }
        }
    }

    let cmdline_script = r#"
for pid in $(ss -tlnp 2>/dev/null | grep -oE 'pid=[0-9]+' | sed 's/pid=//' | sort -u); do
  args=$(tr '\0' ' ' < /proc/$pid/cmdline 2>/dev/null || true)
  echo "$pid $args"
done
"#;
    let mut pid_model: HashMap<u32, String> = HashMap::new();
    if let Some(stdout) = run_shell(ssh_target, cmdline_script).await {
        for line in stdout.lines() {
            let mut it = line.splitn(2, ' ');
            let Some(pid_s) = it.next() else {
                continue;
            };
            let Ok(pid) = pid_s.parse::<u32>() else {
                continue;
            };
            let args = it.next().unwrap_or("");
            let (port, model) = parse_cmdline_port_model(args);
            if let Some(p) = port {
                pid_port.entry(pid).or_insert(p);
            }
            if let Some(m) = model {
                pid_model.insert(pid, m);
            }
        }
    }

    for (pid, (uuid, _)) in pid_primary_uuid {
        if let Some(port) = pid_port.get(&pid) {
            let model = pid_model.get(&pid).cloned();
            out.insert(uuid, (*port, model));
        }
    }

    out
}

/// Map GPU index → discovered port (via UUID join on the same host).
pub async fn discover_gpu_listen_ports(ssh_target: Option<&str>) -> HashMap<u32, (u16, Option<String>)> {
    let by_uuid = discover_gpu_listen_ports_by_uuid(ssh_target).await;
    let mut index_to_uuid: HashMap<u32, String> = HashMap::new();
    let idx_script =
        r#"nvidia-smi --query-gpu=index,uuid --format=csv,noheader,nounits 2>/dev/null || true"#;
    if let Some(stdout) = run_shell(ssh_target, idx_script).await {
        for line in stdout.lines() {
            let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
            if parts.len() >= 2 {
                if let Ok(idx) = parts[0].parse::<u32>() {
                    index_to_uuid.insert(idx, parts[1].to_string());
                }
            }
        }
    }
    let mut out = HashMap::new();
    for (idx, uuid) in index_to_uuid {
        if let Some(entry) = by_uuid.get(&uuid) {
            out.insert(idx, entry.clone());
        }
    }
    out
}

/// Query GPUs on this machine or via `ssh user@host` (fleet tunnel without ncurses).
pub async fn query_gpu_metrics_on_host(host_label: &str, ssh_target: Option<&str>) -> GpuMetrics {
    let smi_args = [
        "--query-gpu=index,uuid,pci.bus_id,serial,name,temperature.gpu,utilization.gpu,utilization.memory,memory.used,memory.total,power.draw,fan.speed",
        "--format=csv,noheader,nounits",
    ];

    let output = if let Some(target) = ssh_target {
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=3",
                target,
                "nvidia-smi",
            ])
            .args(smi_args)
            .output()
            .await
    } else {
        Command::new("nvidia-smi").args(smi_args).output().await
    };

    let output = match output {
        Ok(o) => o,
        Err(e) => {
            return GpuMetrics {
                gpus: Vec::new(),
                error: Some(format!("nvidia-smi failed ({}): {}", host_label, e)),
            };
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return GpuMetrics {
            gpus: Vec::new(),
            error: Some(format!("nvidia-smi error ({}): {}", host_label, stderr)),
        };
    }

    let procs = query_gpu_processes(ssh_target).await;
    let listeners_by_uuid = discover_gpu_listen_ports_by_uuid(ssh_target).await;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut gpus = Vec::new();

    for line in stdout.lines() {
        let parts: Vec<&str> = line.split(',').map(|s| s.trim()).collect();
        if parts.len() >= 9 {
            let idx: u32 = parts[0].parse().unwrap_or(0);
            let uuid = parts.get(1).map(|s| s.to_string());
            let pci = parts.get(2).map(|s| s.to_string());
            let serial = parts
                .get(3)
                .filter(|s| **s != "[N/A]" && !s.is_empty())
                .map(|s| s.to_string());
            let name = parts.get(4).unwrap_or(&"").to_string();
            let (listen_port, cmdline_model) = uuid
                .as_ref()
                .and_then(|u| listeners_by_uuid.get(u))
                .cloned()
                .map(|(p, m)| (Some(p), m))
                .unwrap_or((None, None));
            let mut proc_list = procs.get(&idx).cloned().unwrap_or_default();
            if let Some(port) = listen_port {
                for pr in &mut proc_list {
                    pr.listen_port = Some(port);
                }
            }
            let gpu = GpuInfo {
                index: idx,
                uuid,
                pci_bus_id: pci,
                serial,
                name,
                temperature_c: parts.get(5).and_then(|s| s.parse().ok()).unwrap_or(0),
                utilization_pct: parts.get(6).and_then(|s| s.parse().ok()).unwrap_or(0),
                memory_util_pct: parts.get(7).and_then(|s| s.parse().ok()),
                memory_used_mb: parts.get(8).and_then(|s| s.parse().ok()).unwrap_or(0),
                memory_total_mb: parts.get(9).and_then(|s| s.parse().ok()).unwrap_or(0),
                power_draw_w: parts.get(10).and_then(|s| s.parse().ok()).unwrap_or(0.0),
                fan_speed_pct: parts.get(11).and_then(|s| s.parse().ok()),
                processes: proc_list,
                host: Some(host_label.to_string()),
                node: Some(host_label.to_string()),
                listen_port,
                cmdline_model,
            };
            gpus.push(gpu);
        }
    }

    GpuMetrics { gpus, error: None }
}

/// Per-GPU compute processes (`nvidia-smi pmon`), like nvtop's process column.
async fn query_gpu_processes(ssh_target: Option<&str>) -> HashMap<u32, Vec<GpuProcess>> {
    let mut map: HashMap<u32, Vec<GpuProcess>> = HashMap::new();
    let output = if let Some(target) = ssh_target {
        Command::new("ssh")
            .args([
                "-o",
                "BatchMode=yes",
                "-o",
                "ConnectTimeout=3",
                target,
                "nvidia-smi",
                "pmon",
                "-c",
                "1",
            ])
            .output()
            .await
    } else {
        Command::new("nvidia-smi")
            .args(["pmon", "-c", "1"])
            .output()
            .await
    };

    let Ok(output) = output else { return map };
    if !output.status.success() {
        return map;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 8 {
            continue;
        }
        let gpu_idx: u32 = parts[0].parse().unwrap_or(0);
        let pid: u32 = parts[1].parse().unwrap_or(0);
        let sm_pct = parts[3].parse().unwrap_or(0);
        let mem_pct = parts[4].parse().unwrap_or(0);
        let name = parts[7..].join(" ");
        if name.is_empty() || pid == 0 {
            continue;
        }
        map.entry(gpu_idx).or_default().push(GpuProcess {
            pid,
            name,
            sm_pct,
            mem_pct,
            listen_port: None,
        });
    }
    map
}

/// SSH pull for cesarops2 when node heartbeat has not reported GPUs recently.
pub async fn query_cesarops2_gpus_ssh() -> GpuMetrics {
    query_gpu_metrics_on_host("cesarops2", Some("cesarops@10.0.0.201")).await
}

/// Estimate register pressure from WGSL source.
/// Counts var/let declarations as a heuristic for register usage.
/// P100 rule: >32 registers per thread = occupancy drop.
pub fn estimate_register_pressure(wgsl_source: &str) -> RegisterPressureReport {
    let mut var_count: u32 = 0;
    let mut let_count: u32 = 0;

    for line in wgsl_source.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("var ") || trimmed.starts_with("var<") {
            var_count += 1;
        }
        if trimmed.starts_with("let ") {
            let_count += 1;
        }
    }

    let total = var_count + let_count;
    let warning = if total > 20 {
        Some(format!(
            "High register pressure: {} locals (var={}, let={}). Consider splitting into two dispatches.",
            total, var_count, let_count
        ))
    } else {
        None
    };

    RegisterPressureReport {
        var_count,
        let_count,
        total_locals: total,
        exceeds_threshold: total > 20,
        warning,
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct RegisterPressureReport {
    pub var_count: u32,
    pub let_count: u32,
    pub total_locals: u32,
    pub exceeds_threshold: bool,
    pub warning: Option<String>,
}

/// Check if AVX-512 throttling is likely based on active core count.
/// Xeon Silver 4110: 9+ cores with AVX-512 = frequency penalty.
pub fn avx512_throttle_warning(active_avx512_cores: u32) -> Option<String> {
    if active_avx512_cores >= 9 {
        Some(format!(
            "WARNING: {} cores running AVX-512. Xeon 4110 will throttle ALL cores to 1.4GHz. Limit to 8 cores max.",
            active_avx512_cores
        ))
    } else {
        None
    }
}

/// Merge T440 local nvidia-smi with `all_gpus` from cesarops-node heartbeats (cesarops2, etc.).
pub fn merge_fleet_gpus(
    local: &GpuMetrics,
    node_gpus: Vec<(String, serde_json::Value)>,
) -> Vec<serde_json::Value> {
    let mut out: Vec<serde_json::Value> = local
        .gpus
        .iter()
        .map(|g| serde_json::to_value(g).unwrap_or_default())
        .collect();

    for (node_id, gpu_val) in node_gpus {
        if let Some(arr) = gpu_val.as_array() {
            for g in arr {
                let mut entry = g.clone();
                if let Some(obj) = entry.as_object_mut() {
                    obj.entry("host".to_string())
                        .or_insert(serde_json::json!(node_id));
                    obj.entry("node".to_string())
                        .or_insert(serde_json::json!(node_id));
                }
                out.push(entry);
            }
        }
    }
    out
}

fn node_ids_with_gpus(node_gpus: &[(String, serde_json::Value)]) -> Vec<String> {
    node_gpus
        .iter()
        .filter(|(_, v)| v.as_array().map(|a| !a.is_empty()).unwrap_or(false))
        .map(|(id, _)| id.clone())
        .collect()
}

/// Get a summary of cluster health for the /monitor endpoint and SSE streams.
pub async fn cluster_summary(node_gpus: Vec<(String, serde_json::Value)>) -> serde_json::Value {
    let gpu = query_gpu_metrics().await;
    let mut merged = merge_fleet_gpus(&gpu, node_gpus.clone());

    // Tunnel remote GPUs (nvtop-style fleet view) when cesarops-node heartbeats are missing.
    let have_remote = node_ids_with_gpus(&node_gpus)
        .iter()
        .any(|id| id.contains("cesarops2") || id.contains("201"));
    if !have_remote {
        let remote = query_cesarops2_gpus_ssh().await;
        if !remote.gpus.is_empty() {
            for g in &remote.gpus {
                merged.push(serde_json::to_value(g).unwrap_or_default());
            }
        }
    }

    serde_json::json!({
        "source": "nvml",
        "source_note": "Live metrics via nvidia-smi (same NVML as nvtop). Stream: GET /gpu/stream",
        "ts": chrono_lite_now(),
        "gpus": merged,
        "gpu_error": gpu.error,
        "avx512_note": "Limit AVX-512 to 8 cores (Socket 0) to avoid throttle",
        "numa": {
            "socket_0": "Cores 0-7, P100 #0, DDR4 Ch 0-2",
            "socket_1": "Cores 8-15, P100 #1, DDR4 Ch 3-5",
        },
    })
}

fn chrono_lite_now() -> String {
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}", d.as_secs())
}
