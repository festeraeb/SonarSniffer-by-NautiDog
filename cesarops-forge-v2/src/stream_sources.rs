//! Whitelisted live output streams for the Forge main page (SSE).

use crate::AppState;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::stream::{self, StreamExt};
use std::collections::VecDeque;
use std::convert::Infallible;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::{broadcast, Mutex};

const ACTIVITY_CAP: usize = 400;
const SSE_TAIL_MS: u64 = 250;

#[derive(Clone, Debug)]
pub struct ActivityLine {
    pub ts: String,
    pub level: String,
    pub message: String,
}

#[derive(Clone)]
pub struct StreamLog {
    buffer: Arc<Mutex<VecDeque<ActivityLine>>>,
    tx: broadcast::Sender<ActivityLine>,
}

impl StreamLog {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(256);
        Self {
            buffer: Arc::new(Mutex::new(VecDeque::new())),
            tx,
        }
    }

    pub async fn push(&self, level: &str, message: impl Into<String>) {
        let line = ActivityLine {
            ts: now_hms(),
            level: level.to_string(),
            message: message.into(),
        };
        let mut buf = self.buffer.lock().await;
        buf.push_back(line.clone());
        while buf.len() > ACTIVITY_CAP {
            buf.pop_front();
        }
        drop(buf);
        let _ = self.tx.send(line);
    }

    pub async fn snapshot(&self) -> Vec<ActivityLine> {
        self.buffer.lock().await.iter().cloned().collect()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<ActivityLine> {
        self.tx.subscribe()
    }
}

pub fn now_hms() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let h = (secs / 3600) % 24;
    let m = (secs / 60) % 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[derive(Clone, Copy)]
struct StreamDef {
    id: &'static str,
    label: &'static str,
    kind: &'static str,
    description: &'static str,
}

const STREAMS: &[StreamDef] = &[
    StreamDef {
        id: "activity",
        label: "Forge activity (tools + send)",
        kind: "activity",
        description: "Live tool calls, user sends, and responses from this Forge instance",
    },
    StreamDef {
        id: "journal",
        label: "Forge service (journalctl)",
        kind: "journal",
        description: "systemd journal for cesarops-forge-v2",
    },
    StreamDef {
        id: "gpu",
        label: "Fleet GPUs (NVML / nvtop-class)",
        kind: "gpu",
        description: "T440 + cesarops2 GPU util, memory, power, processes (~2s)",
    },
    StreamDef {
        id: "hub",
        label: "Fleet hub (mode + orch)",
        kind: "hub",
        description: "Fleet mode, orchestration backend, GPU summary",
    },
    StreamDef {
        id: "gpu-poll",
        label: "GPU poll log (/tmp)",
        kind: "file",
        description: "Background gpu-poll-watch.log if present",
    },
    StreamDef {
        id: "coder-log",
        label: "Coder / preset log (/tmp)",
        kind: "file",
        description: "preset_gpu0.log or llama-server launch log",
    },
    StreamDef {
        id: "nautivecs-index",
        label: "nautivecs index log",
        kind: "file",
        description: "/tmp/nautivecs-index.log",
    },
];

fn find_stream(id: &str) -> Option<&'static StreamDef> {
    STREAMS.iter().find(|s| s.id == id)
}

pub fn list_streams_json() -> serde_json::Value {
    serde_json::json!({
        "streams": STREAMS.iter().map(|s| serde_json::json!({
            "id": s.id,
            "label": s.label,
            "kind": s.kind,
            "description": s.description,
            "url": format!("/streams/{}", s.id),
        })).collect::<Vec<_>>()
    })
}

pub async fn stream_sse(
    id: &str,
    state: AppState,
) -> Result<Sse<impl stream::Stream<Item = Result<Event, Infallible>>>, &'static str> {
    let def = find_stream(id).ok_or("unknown stream")?;

    let s: SseStream = match def.kind {
        "activity" => activity_stream(state.stream_log.clone()),
        "journal" => {
            let mut cmd = Command::new("journalctl");
            cmd.args([
                "-u",
                "cesarops-forge-v2",
                "-f",
                "-n",
                "60",
                "--no-pager",
                "-o",
                "short-iso",
            ]);
            command_line_stream(cmd).await?
        }
        "gpu" => gpu_text_stream(state).await?,
        "hub" => hub_text_stream(state).await?,
        "file" => {
            let path = resolve_file_path(def.id).ok_or("file not configured")?;
            file_tail_stream(&path).await?
        }
        _ => return Err("unsupported"),
    };

    Ok(Sse::new(s).keep_alive(KeepAlive::default()))
}

fn resolve_file_path(id: &str) -> Option<String> {
    match id {
        "gpu-poll" => Some("/tmp/gpu-poll-watch.log".to_string()),
        "coder-log" => {
            if Path::new("/tmp/preset_gpu0.log").exists() {
                Some("/tmp/preset_gpu0.log".to_string())
            } else if Path::new("/tmp/llama-server.log").exists() {
                Some("/tmp/llama-server.log".to_string())
            } else {
                Some("/tmp/preset_gpu0.log".to_string())
            }
        }
        "nautivecs-index" => Some("/tmp/nautivecs-index.log".to_string()),
        _ => None,
    }
}

type SseStream = std::pin::Pin<Box<dyn stream::Stream<Item = Result<Event, Infallible>> + Send>>;

fn sse_text(line: &str) -> Result<Event, Infallible> {
    Ok(Event::default().data(line.to_string()))
}

fn format_activity_line(l: &ActivityLine) -> String {
    format!("[{}] {} {}", l.ts, l.level, l.message)
}

fn activity_stream(log: StreamLog) -> SseStream {
    let log_backlog = log.clone();
    let initial = stream::once(async move {
        let snap = log_backlog.snapshot().await;
        let text = if snap.is_empty() {
            format!("# activity stream connected {}\n", now_hms())
        } else {
            snap.iter().map(format_activity_line).collect::<Vec<_>>().join("\n")
        };
        Ok(sse_text(&text).unwrap())
    });

    let live = stream::unfold(log.subscribe(), move |mut rx| async move {
        loop {
            match rx.recv().await {
                Ok(line) => {
                    tokio::time::sleep(std::time::Duration::from_millis(SSE_TAIL_MS)).await;
                    return Some((Ok(sse_text(&format_activity_line(&line)).unwrap()), rx));
                }
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });

    Box::pin(initial.chain(live))
}

async fn command_line_stream(mut cmd: Command) -> Result<SseStream, &'static str> {
    cmd.kill_on_drop(true);
    let mut child = cmd
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|_| "spawn failed")?;

    let stdout = child.stdout.take().ok_or("no stdout")?;
    let stderr = child.stderr.take().ok_or("no stderr")?;
    let reader_out = BufReader::new(stdout);
    let reader_err = BufReader::new(stderr);

    let (tx, rx) = tokio::sync::mpsc::channel::<String>(128);
    let tx_err = tx.clone();
    tokio::spawn(async move {
        let mut lines = reader_out.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx.send(line).await;
        }
    });
    tokio::spawn(async move {
        let mut lines = reader_err.lines();
        while let Ok(Some(line)) = lines.next_line().await {
            let _ = tx_err.send(format!("[stderr] {}", line)).await;
        }
    });

    Ok(Box::pin(stream::unfold(rx, move |mut rx| async move {
        match rx.recv().await {
            Some(line) => {
                tokio::time::sleep(std::time::Duration::from_millis(SSE_TAIL_MS)).await;
                Some((Ok(sse_text(&line).unwrap()), rx))
            }
            None => None,
        }
    })))
}

async fn file_tail_stream(path: &str) -> Result<SseStream, &'static str> {
    if !Path::new(path).exists() {
        let msg = format!("# waiting for {} — file not created yet\n", path);
        return Ok(Box::pin(stream::once(async move {
            Ok(sse_text(&msg).unwrap())
        })));
    }
    let mut cmd = Command::new("tail");
    cmd.args(["-n", "40", "-F", path]);
    command_line_stream(cmd).await
}

async fn gpu_text_stream(state: AppState) -> Result<SseStream, &'static str> {
    Ok(Box::pin(stream::unfold(state, move |state| {
        async move {
            let node_gpus = crate::collect_node_gpus(&state).await;
            let mon = crate::hardware::cluster_summary(node_gpus).await;
            let text = format_gpu_monitor(&mon);
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            Some((Ok(sse_text(&text).unwrap()), state))
        }
    })))
}

async fn hub_text_stream(state: AppState) -> Result<SseStream, &'static str> {
    Ok(Box::pin(stream::unfold(state, move |state| {
        async move {
            let node_gpus = crate::collect_node_gpus(&state).await;
            let mon = crate::hardware::cluster_summary(node_gpus).await;
            let mode = std::fs::read_to_string("/home/cesarops/.cache/cesarops/fleet_mode")
                .unwrap_or_else(|_| "normal".into());
            let orch = crate::orchestration::load_orchestration().tools_backend;
            let text = format!(
                "[{}] fleet_mode={} tools_backend={}\n{}",
                now_hms(),
                mode.trim(),
                orch,
                format_gpu_monitor(&mon)
            );
            tokio::time::sleep(std::time::Duration::from_millis(2000)).await;
            Some((Ok(sse_text(&text).unwrap()), state))
        }
    })))
}

fn format_gpu_monitor(mon: &serde_json::Value) -> String {
    let mut lines = vec![format!("--- GPU tick {} ---", now_hms())];
    if let Some(err) = mon.get("gpu_error").and_then(|v| v.as_str()) {
        if !err.is_empty() {
            lines.push(format!("error: {}", err));
        }
    }
    if let Some(arr) = mon.get("gpus").and_then(|v| v.as_array()) {
        for g in arr {
            let host = g.get("host").and_then(|v| v.as_str()).unwrap_or("?");
            let idx = g.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
            let name = g.get("name").and_then(|v| v.as_str()).unwrap_or("GPU");
            let util = g.get("utilization_pct").and_then(|v| v.as_u64()).unwrap_or(0);
            let mem_u = g.get("memory_used_mb").and_then(|v| v.as_u64()).unwrap_or(0);
            let mem_t = g.get("memory_total_mb").and_then(|v| v.as_u64()).unwrap_or(0);
            let temp = g.get("temperature_c").and_then(|v| v.as_u64()).unwrap_or(0);
            let power = g.get("power_draw_w").and_then(|v| v.as_f64()).unwrap_or(0.0);
            lines.push(format!(
                "{host} #{idx} {name} | {util}% SM | {mem_u}/{mem_t} MiB | {temp}°C | {power:.0}W"
            ));
            if let Some(procs) = g.get("processes").and_then(|v| v.as_array()) {
                for p in procs.iter().take(3) {
                    let pname = p.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                    let sm = p.get("sm_pct").and_then(|v| v.as_u64()).unwrap_or(0);
                    lines.push(format!("    └ {pname} sm={sm}%"));
                }
            }
        }
    }
    lines.join("\n")
}

/// Push from tools / loop (public helper).
pub async fn log_tool(state: &AppState, name: &str, detail: &str) {
    state
        .stream_log
        .push("tool", format!("{name}: {}", truncate(detail, 240)))
        .await;
}

pub async fn log_send(state: &AppState, phase: &str, detail: &str) {
    state
        .stream_log
        .push("send", format!("{phase}: {}", truncate(detail, 200)))
        .await;
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", &s[..end])
}
