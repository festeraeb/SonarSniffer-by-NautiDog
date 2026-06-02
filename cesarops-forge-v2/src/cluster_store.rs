//! Read/write cluster_config.toml and GPU ↔ worker bindings.

use serde_json::json;
use tracing::info;

use crate::paths;

pub fn read_config() -> toml::Table {
    std::fs::read_to_string(paths::cluster_config_path())
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_default()
}

pub fn write_config(table: &toml::Table) -> Result<(), String> {
    let s = toml::to_string_pretty(table).map_err(|e| e.to_string())?;
    std::fs::write(paths::cluster_config_path(), s).map_err(|e| e.to_string())
}

fn worker_for_gpu<'a>(workers: &'a [toml::Value], gpu_id: i64) -> Option<&'a toml::Table> {
    workers.iter().find_map(|w| {
        let t = w.as_table()?;
        if t.get("gpu").and_then(|v| v.as_integer()) == Some(gpu_id) {
            Some(t)
        } else {
            None
        }
    })
}

/// Merge [[gpu]] inventory with bound [[worker]] row (if any).
pub fn gpu_fleet_json() -> serde_json::Value {
    let table = read_config();
    let gpus = table
        .get("gpu")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();
    let workers = table
        .get("worker")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let cards: Vec<serde_json::Value> = gpus
        .iter()
        .filter_map(|g| {
            let gt = g.as_table()?;
            let id = gt.get("id").and_then(|v| v.as_integer()).unwrap_or(0);
            let w = worker_for_gpu(&workers, id);
            let host = gt.get("host").and_then(|v| v.as_str()).unwrap_or("local");
            let port_base = gt.get("port_base").and_then(|v| v.as_integer()).unwrap_or(5010 + id);
            Some(json!({
                "id": id,
                "name": gt.get("name").and_then(|v| v.as_str()).unwrap_or("GPU"),
                "node": gt.get("node").and_then(|v| v.as_str()).unwrap_or(""),
                "bus_id": gt.get("bus_id").and_then(|v| v.as_str()).unwrap_or(""),
                "gpu_uuid": gt.get("gpu_uuid").and_then(|v| v.as_str()).unwrap_or(""),
                "vram_mb": gt.get("vram_mb").and_then(|v| v.as_integer()).unwrap_or(0),
                "host": host,
                "remote": host != "local",
                "cuda_index": gt.get("cuda_index").and_then(|v| v.as_integer()),
                "port_hint": port_base,
                "notes": gt.get("notes").and_then(|v| v.as_str()).unwrap_or(""),
                "worker_name": w.and_then(|t| t.get("name")).and_then(|v| v.as_str()).unwrap_or(""),
                "role": w.and_then(|t| t.get("role")).and_then(|v| v.as_str()).unwrap_or("idle"),
                "model": w.and_then(|t| t.get("model")).and_then(|v| v.as_str()).unwrap_or(""),
                "engine": w.and_then(|t| t.get("engine")).and_then(|v| v.as_str()).unwrap_or("llama-server"),
                "backend": w.and_then(|t| t.get("backend")).and_then(|v| v.as_str()).unwrap_or("cuda"),
                "port": w.and_then(|t| t.get("port")).and_then(|v| v.as_integer()).unwrap_or(port_base),
                "enabled": w.and_then(|t| t.get("enabled")).and_then(|v| v.as_bool()).unwrap_or(false),
            }))
        })
        .collect();

    json!({ "gpus": cards, "roles": crate::routing::LANE_ROLE_CATALOG.iter().map(|(id,l)| json!({"id": id, "label": l})).collect::<Vec<_>>() })
}

pub fn apply_gpu_binding(
    gpu_id: i64,
    role: &str,
    model: &str,
    engine: &str,
    backend: &str,
) -> Result<String, String> {
    let mut table = read_config();

    let (gpu_name, gpu_host) = table
        .get("gpu")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|g| {
                let t = g.as_table()?;
                if t.get("id").and_then(|v| v.as_integer()) == Some(gpu_id) {
                    Some((
                        t.get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("GPU")
                            .to_string(),
                        t.get("host")
                            .and_then(|v| v.as_str())
                            .unwrap_or("local")
                            .to_string(),
                    ))
                } else {
                    None
                }
            })
        })
        .unwrap_or_else(|| (format!("GPU{}", gpu_id), "local".to_string()));

    let port = table
        .get("gpu")
        .and_then(|v| v.as_array())
        .and_then(|arr| {
            arr.iter().find_map(|g| {
                let t = g.as_table()?;
                if t.get("id").and_then(|v| v.as_integer()) == Some(gpu_id) {
                    t.get("port_base").and_then(|v| v.as_integer())
                } else {
                    None
                }
            })
        })
        .unwrap_or(5010 + gpu_id);

    let worker_name = match gpu_id {
        2 => "RTX2060".to_string(),
        3 => "GTX1070".to_string(),
        4 => "M2200".to_string(),
        _ => format!("gpu{}-{}", gpu_id, role),
    };

    let workers = table
        .get_mut("worker")
        .and_then(|v| v.as_array_mut())
        .ok_or("No [[worker]] section in config")?;

    let idx = workers.iter().position(|w| {
        w.as_table()
            .and_then(|t| t.get("gpu"))
            .and_then(|v| v.as_integer())
            == Some(gpu_id)
    });

    let mut row = toml::Table::new();
    row.insert("name".into(), toml::Value::String(worker_name.clone()));
    row.insert("role".into(), toml::Value::String(role.to_string()));
    row.insert("gpu".into(), toml::Value::Integer(gpu_id));
    row.insert("model".into(), toml::Value::String(model.to_string()));
    row.insert("port".into(), toml::Value::Integer(port));
    row.insert("template".into(), toml::Value::String("qwen2.5".into()));
    row.insert("enabled".into(), toml::Value::Boolean(false));
    row.insert("engine".into(), toml::Value::String(engine.to_string()));
    row.insert("backend".into(), toml::Value::String(backend.to_string()));
    row.insert("inject_vectors".into(), toml::Value::Boolean(true));
    row.insert("memory_pool".into(), toml::Value::String(String::new()));
    row.insert("host".into(), toml::Value::String(gpu_host));

    let entry = toml::Value::Table(row);
    if let Some(i) = idx {
        workers[i] = entry;
    } else {
        workers.push(entry);
    }

    write_config(&table)?;
    info!("GPU {} ({}) bound: role={} engine={}", gpu_id, gpu_name, role, engine);
    Ok(format!(
        "GPU {} ({}) → {} / {} on port {}",
        gpu_id, gpu_name, role, engine, port
    ))
}

pub fn update_worker_config(name: &str, config: &serde_json::Value) -> Result<(), String> {
    let mut table = read_config();
    let workers = table
        .get_mut("worker")
        .and_then(|v| v.as_array_mut())
        .ok_or("No workers")?;

    let idx = workers.iter().position(|w| {
        w.as_table()
            .and_then(|t| t.get("name"))
            .and_then(|v| v.as_str())
            == Some(name)
    }).ok_or_else(|| format!("Worker not found: {}", name))?;

    let row = workers[idx].as_table_mut().ok_or("Invalid worker row")?;

    for (key, val) in config.as_object().unwrap_or(&serde_json::Map::new()) {
        if key == "corrector_functions" {
            continue;
        }
        if let Some(s) = val.as_str() {
            row.insert(key.clone(), toml::Value::String(s.to_string()));
        } else if let Some(b) = val.as_bool() {
            row.insert(key.clone(), toml::Value::Boolean(b));
        } else if let Some(n) = val.as_u64() {
            row.insert(key.clone(), toml::Value::Integer(n as i64));
        } else if key == "port" {
            if let Some(n) = val.as_i64() {
                row.insert(key.clone(), toml::Value::Integer(n));
            }
        }
    }

    write_config(&table)?;
    info!("Persisted worker {}", name);
    Ok(())
}

pub fn update_worker_field(name: &str, field: &str, value: serde_json::Value) -> Result<(), String> {
    let mut patch = serde_json::Map::new();
    patch.insert(field.to_string(), value);
    update_worker_config(name, &serde_json::Value::Object(patch))
}

pub fn resolve_worker_idx(workers: &[toml::Value], path: &str) -> Option<usize> {
    if let Ok(n) = path.parse::<usize>() {
        if n < workers.len() {
            return Some(n);
        }
    }
    workers.iter().position(|w| {
        w.as_table()
            .and_then(|t| t.get("name"))
            .and_then(|v| v.as_str())
            == Some(path)
    })
}

pub fn worker_row(path: &str) -> Option<(usize, toml::Table)> {
    let table = read_config();
    let workers = table.get("worker")?.as_array()?;
    let idx = resolve_worker_idx(workers, path)?;
    let row = workers[idx].as_table()?.clone();
    Some((idx, row))
}

pub async fn probe_endpoint(url: &str) -> serde_json::Value {
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(3))
        .build()
        .unwrap();
    let health_url = format!("{}/health", url.trim_end_matches('/'));
    let models_url = format!("{}/v1/models", url.trim_end_matches('/'));

    for u in [health_url, models_url] {
        if let Ok(resp) = client.get(&u).send().await {
            if resp.status().is_success() {
                let body: serde_json::Value = resp.json().await.unwrap_or(json!({"ok": true}));
                return json!({ "online": true, "url": url, "probe": u, "info": body });
            }
        }
    }
    json!({ "online": false, "url": url })
}

pub fn full_panel_config() -> serde_json::Value {
    let table = read_config();
    let workers: Vec<serde_json::Value> = table
        .get("worker")
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .map(|w| {
                    let t = w.as_table().cloned().unwrap_or_default();
                    json!({
                        "name": t.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                        "role": t.get("role").and_then(|v| v.as_str()).unwrap_or(""),
                        "node_ip": if t.get("host").and_then(|v| v.as_str()) == Some("local") {
                            "127.0.0.1"
                        } else {
                            t.get("host").and_then(|v| v.as_str()).unwrap_or("127.0.0.1")
                        },
                        "port": t.get("port").and_then(|v| v.as_integer()).unwrap_or(5001),
                        "engine": t.get("engine").and_then(|v| v.as_str()).unwrap_or("llama-server"),
                        "backend": t.get("backend").and_then(|v| v.as_str()).unwrap_or("cuda"),
                        "inject_vectors": t.get("inject_vectors").and_then(|v| v.as_bool()).unwrap_or(true),
                        "memory_pool": t.get("memory_pool").and_then(|v| v.as_str()).unwrap_or(""),
                        "model": t.get("model").and_then(|v| v.as_str()).unwrap_or(""),
                        "gpu": t.get("gpu").and_then(|v| v.as_integer()).unwrap_or(0),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    json!({ "workers": workers, "pools": [], "gpus": gpu_fleet_json().get("gpus").cloned().unwrap_or_default() })
}

/// Stop inference locally and reset every [[worker]] to idle (keeps GPU rows).
pub fn clear_all_fleet_bindings() -> Result<usize, String> {
    let mut table = read_config();
    let workers = table
        .get_mut("worker")
        .and_then(|v| v.as_array_mut())
        .ok_or("No [[worker]] section")?;
    let mut n = 0usize;
    for w in workers.iter_mut() {
        if let Some(t) = w.as_table_mut() {
            t.insert("role".into(), toml::Value::String("idle".into()));
            t.insert("model".into(), toml::Value::String(String::new()));
            t.insert("enabled".into(), toml::Value::Boolean(false));
            n += 1;
        }
    }
    write_config(&table)?;
    Ok(n)
}

/// Persist NVML `gpu_uuid` (and full `bus_id` when known) from live fleet probe into [[gpu]] rows.
pub fn sync_gpu_uuids_from_live(live_gpus: &[serde_json::Value]) -> Result<Vec<String>, String> {
    let mut table = read_config();
    let gpus = table
        .get_mut("gpu")
        .and_then(|v| v.as_array_mut())
        .ok_or("No [[gpu]] section in cluster_config.toml")?;
    let mut updated = Vec::new();
    for live in live_gpus {
        let uuid = live
            .get("gpu_uuid")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && !s.starts_with("config-"));
        let Some(uuid) = uuid else { continue };
        let id = live.get("id").and_then(|v| v.as_i64());
        let pci = live
            .get("pci_bus_id")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty());
        let Some(id) = id else { continue };
        for g in gpus.iter_mut() {
            let Some(t) = g.as_table_mut() else { continue };
            if t.get("id").and_then(|v| v.as_integer()) != Some(id) {
                continue;
            }
            let prev = t
                .get("gpu_uuid")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            t.insert("gpu_uuid".into(), toml::Value::String(uuid.to_string()));
            if let Some(p) = pci {
                t.insert("bus_id".into(), toml::Value::String(p.to_string()));
            }
            if prev != uuid {
                updated.push(format!("gpu id {} ({}) → {}", id, live.get("name").and_then(|v| v.as_str()).unwrap_or("?"), uuid));
            }
        }
    }
    if updated.is_empty() {
        return Ok(updated);
    }
    write_config(&table)?;
    Ok(updated)
}
