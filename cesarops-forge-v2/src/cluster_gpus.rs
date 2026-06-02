//! Unified cluster GPU view — identity-first (NVML UUID / PCIe bus_id), dynamic ports.

use std::collections::HashMap;

use crate::{cluster_store, hardware, AppState};

fn forge_host_ip() -> &'static str {
    let hn = std::fs::read_to_string("/etc/hostname").unwrap_or_default().to_lowercase();
    if hn.contains("t440") {
        "10.0.0.61"
    } else {
        "10.0.0.201"
    }
}

fn infer_node(host: &str, node_field: Option<&str>) -> String {
    if let Some(n) = node_field.filter(|s| !s.is_empty()) {
        return n.to_string();
    }
    match host {
        "local" | "127.0.0.1" => {
            if forge_host_ip() == "10.0.0.61" {
                "t440cesarops".into()
            } else {
                "cesarops2".into()
            }
        }
        "10.0.0.61" | "100.72.182.77" => "t440cesarops".into(),
        "10.0.0.201" | "100.72.129.86" => "cesarops2".into(),
        "100.110.214.86" => "nautik9".into(),
        "100.105.77.74" => "cesarops3".into(),
        other => other.to_string(),
    }
}

async fn probe_models(
    client: &reqwest::Client,
    host_ip: &str,
    port: i64,
) -> (bool, Option<String>) {
    let base = format!("http://{}:{}", host_ip, port);
    let models_url = format!("{}/v1/models", base);
    if let Ok(resp) = client.get(&models_url).send().await {
        if resp.status().is_success() {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                if let Some(id) = v
                    .get("data")
                    .and_then(|d| d.as_array())
                    .and_then(|a| a.first())
                    .and_then(|m| m.get("id"))
                    .and_then(|id| id.as_str())
                {
                    if !id.is_empty() && id != "000" {
                        return (true, Some(id.to_string()));
                    }
                }
            }
            return (true, None);
        }
    }
    for path in ["/health", "/api/extra/version"] {
        if let Ok(resp) = client.get(format!("{}{}", base, path)).send().await {
            if resp.status().is_success() {
                return (true, None);
            }
        }
    }
    (false, None)
}

fn live_gpu_to_card(
    g: &hardware::GpuInfo,
    node: &str,
    host_ip: &str,
    port: i64,
    online: bool,
    loaded_model: String,
) -> serde_json::Value {
    serde_json::json!({
        "id": g.index,
        "gpu_uuid": g.uuid.clone().unwrap_or_default(),
        "pci_bus_id": g.pci_bus_id.clone().unwrap_or_default(),
        "serial": g.serial.clone().unwrap_or_default(),
        "identity_key": g.uuid.as_ref().map(|u| hardware::gpu_identity_key(node, u)),
        "name": g.name,
        "node": node,
        "host": host_ip,
        "cuda_index": g.index as i64,
        "port": port,
        "port_discovered": g.listen_port.is_some(),
        "port_hint": port,
        "vram_mb": g.memory_total_mb,
        "memory_used_mb": g.memory_used_mb,
        "memory_total_mb": g.memory_total_mb,
        "temperature_c": g.temperature_c,
        "utilization_pct": g.utilization_pct,
        "processes": g.processes,
        "live": true,
        "endpoint_online": online,
        "loaded_model": loaded_model,
        "cmdline_model": g.cmdline_model.clone().unwrap_or_default(),
        "role": "idle",
        "model": "",
        "engine": "llama-server",
        "remote": host_ip != forge_host_ip(),
    })
}

fn config_matches_live(
    cfg: &serde_json::Value,
    g: &hardware::GpuInfo,
    node: &str,
) -> bool {
    let cfg_node = infer_node(
        cfg.get("host").and_then(|v| v.as_str()).unwrap_or("local"),
        cfg.get("node").and_then(|v| v.as_str()),
    );
    if cfg_node != node {
        return false;
    }
    if let (Some(cu), Some(u)) = (
        cfg.get("gpu_uuid").and_then(|v| v.as_str()).filter(|s| !s.is_empty()),
        g.uuid.as_deref(),
    ) {
        if cu == u {
            return true;
        }
    }
    if let (Some(cb), Some(lb)) = (
        cfg.get("bus_id").and_then(|v| v.as_str()),
        g.pci_bus_id.as_deref(),
    ) {
        if hardware::pci_bus_ids_match(cb, lb) {
            return true;
        }
    }
    if let (Some(cc), Some(ci)) = (
        cfg.get("cuda_index").and_then(|v| v.as_i64()),
        Some(g.index as i64),
    ) {
        if cc == ci {
            return true;
        }
    }
    false
}

fn overlay_config(card: &mut serde_json::Map<String, serde_json::Value>, cfg: &serde_json::Value) {
    for key in [
        "id",
        "name",
        "notes",
        "worker_name",
        "role",
        "model",
        "engine",
        "backend",
    ] {
        if let Some(v) = cfg.get(key) {
            if key == "role" || key == "model" {
                if v.as_str().map(|s| !s.is_empty() && s != "idle").unwrap_or(false) {
                    card.insert(key.to_string(), v.clone());
                }
            } else if key == "name" {
                card.insert(key.to_string(), v.clone());
            } else if key != "model" || v.as_str().map(|s| !s.is_empty()).unwrap_or(false) {
                card.insert(key.to_string(), v.clone());
            }
        }
    }
    if let Some(bus) = cfg.get("bus_id").and_then(|v| v.as_str()) {
        card.insert("bus_id".into(), serde_json::json!(bus));
    }
}

/// Build merged GPU list — one row per physical GPU (UUID), port discovered at runtime.
pub async fn unified_fleet_json(state: &AppState) -> serde_json::Value {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(4))
        .build()
        .unwrap();

    let base = cluster_store::gpu_fleet_json();
    let config_cards: Vec<serde_json::Value> = base
        .get("gpus")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut by_uuid: HashMap<String, serde_json::Value> = HashMap::new();
    let mut matched_config: Vec<bool> = vec![false; config_cards.len()];

    async fn ingest_host(
        by_uuid: &mut HashMap<String, serde_json::Value>,
        config_cards: &[serde_json::Value],
        matched_config: &mut [bool],
        client: &reqwest::Client,
        node: &str,
        host_ip: &str,
        ssh: Option<&str>,
    ) {
        let smi = hardware::query_gpu_metrics_on_host(node, ssh).await;
        for g in &smi.gpus {
            let uuid = match &g.uuid {
                Some(u) if !u.is_empty() => u.clone(),
                _ => continue,
            };
            let port = g.listen_port.unwrap_or(0) as i64;
            let port = if port > 0 {
                port
            } else {
                // fallback: config port_hint for this identity
                config_cards
                    .iter()
                    .find(|c| config_matches_live(c, g, node))
                    .and_then(|c| c.get("port").and_then(|v| v.as_i64()))
                    .unwrap_or(0)
            };
            let (online, api_model) = if port > 0 {
                probe_models(client, host_ip, port).await
            } else {
                (false, None)
            };
            let loaded = api_model
                .or_else(|| g.cmdline_model.clone())
                .unwrap_or_default();
            let key = hardware::gpu_identity_key(node, &uuid);
            let mut card = live_gpu_to_card(g, node, host_ip, port, online, loaded);
            if let Some(obj) = card.as_object_mut() {
                for (i, cfg) in config_cards.iter().enumerate() {
                    if config_matches_live(cfg, g, node) {
                        overlay_config(obj, cfg);
                        matched_config[i] = true;
                    }
                }
            }
            by_uuid.insert(key, card);
        }
    }

    let forge_ip = forge_host_ip();
    let forge_node = infer_node(forge_ip, None);
    ingest_host(
        &mut by_uuid,
        &config_cards,
        &mut matched_config,
        &client,
        &forge_node,
        forge_ip,
        None,
    )
    .await;

    if forge_ip != "10.0.0.61" {
        ingest_host(
            &mut by_uuid,
            &config_cards,
            &mut matched_config,
            &client,
            "t440cesarops",
            "10.0.0.61",
            Some("cesarops@10.0.0.61"),
        )
        .await;
    }

    // Config-only rows (no local smi on forge host, e.g. nautik9)
    for (i, cfg) in config_cards.iter().enumerate() {
        if matched_config[i] {
            continue;
        }
        let host = cfg.get("host").and_then(|v| v.as_str()).unwrap_or("local");
        let node = infer_node(host, cfg.get("node").and_then(|v| v.as_str()));
        let host_ip = if host == "local" {
            forge_host_ip().to_string()
        } else {
            host.to_string()
        };
        let port_hint = cfg
            .get("port")
            .or_else(|| cfg.get("port_base"))
            .and_then(|v| v.as_i64())
            .unwrap_or(0);
        let (online, loaded) = if port_hint > 0 {
            probe_models(&client, &host_ip, port_hint).await
        } else {
            (false, None)
        };
        let uuid = cfg
            .get("gpu_uuid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| {
                format!(
                    "config-{}-{}",
                    node,
                    cfg.get("id").and_then(|v| v.as_i64()).unwrap_or(i as i64)
                )
            });
        let key = hardware::gpu_identity_key(&node, &uuid);
        let mut card = cfg.clone();
        if let Some(obj) = card.as_object_mut() {
            obj.insert("node".into(), serde_json::json!(node));
            obj.insert("host".into(), serde_json::json!(host_ip));
            obj.insert("gpu_uuid".into(), serde_json::json!(uuid));
            obj.insert("identity_key".into(), serde_json::json!(key));
            obj.insert("port".into(), serde_json::json!(port_hint));
            obj.insert("port_discovered".into(), serde_json::json!(false));
            obj.insert("endpoint_online".into(), serde_json::json!(online));
            obj.insert(
                "loaded_model".into(),
                serde_json::json!(loaded.unwrap_or_else(|| {
                    cfg.get("model")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string()
                })),
            );
            obj.insert("live".into(), serde_json::json!(false));
        }
        by_uuid.insert(key, card);
    }

    let registry = state.node_registry.lock().await;
    let heartbeat_nodes = registry.len();
    drop(registry);

    let mut gpus: Vec<serde_json::Value> = by_uuid.into_values().collect();
    gpus.sort_by(|a, b| {
        let na = a.get("node").and_then(|v| v.as_str()).unwrap_or("");
        let nb = b.get("node").and_then(|v| v.as_str()).unwrap_or("");
        na.cmp(nb).then_with(|| {
            a.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .cmp(b.get("name").and_then(|v| v.as_str()).unwrap_or(""))
        })
    });

    serde_json::json!({
        "gpus": gpus,
        "forge_host": forge_host_ip(),
        "heartbeat_nodes": heartbeat_nodes,
        "identity_mode": "gpu_uuid",
        "roles": base.get("roles").cloned().unwrap_or_default(),
    })
}
