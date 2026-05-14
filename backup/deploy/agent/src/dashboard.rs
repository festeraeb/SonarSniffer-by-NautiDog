use axum::extract::ws::{WebSocket, WebSocketUpgrade};
use axum::response::{Html, IntoResponse, Response};
use axum::Json;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

pub const DASHBOARD_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>CesarOps Dashboard</title>
    <style>
        body { font-family: system-ui, sans-serif; background: #0f172a; color: #e2e8f0; margin: 0; padding: 20px; }
        .grid { display: grid; grid-template-columns: repeat(auto-fit, minmax(300px, 1fr)); gap: 20px; }
        .card { background: #1e293b; padding: 20px; border-radius: 12px; box-shadow: 0 4px 6px rgba(0,0,0,0.3); }
        .status-light { width: 12px; height: 12px; border-radius: 50%; display: inline-block; margin-right: 8px; }
        .green { background: #22c55e; } .yellow { background: #eab308; } .red { background: #ef4444; }
        .node { padding: 10px; margin: 5px 0; background: #334155; border-radius: 6px; font-size: 0.9em; }
        .log { max-height: 200px; overflow-y: auto; background: #0f172a; padding: 10px; border-radius: 6px; font-family: monospace; }
        button { background: #3b82f6; color: white; border: none; padding: 8px 16px; border-radius: 6px; cursor: pointer; margin: 4px; }
        button:hover { background: #2563eb; }
    </style>
</head>
<body>
    <h1>CesarOps Agent</h1>
    <div class="grid">
        <div class="card">
            <h2>Cluster Health</h2>
            <div id="health"></div>
        </div>
        <div class="card">
            <h2>Workflow Graph</h2>
            <div id="graph"></div>
        </div>
        <div class="card">
            <h2>Recent Decisions</h2>
            <div id="decisions" class="log"></div>
        </div>
        <div class="card">
            <h2>Manual Triggers</h2>
            <div id="triggers"></div>
        </div>
    </div>
    <script>
        const ws = new WebSocket(`ws://${location.host}/ws`);
        ws.onmessage = (e) => {
            const data = JSON.parse(e.data);
            if (data.type === 'event') {
                const div = document.createElement('div');
                div.className = 'node';
                div.textContent = `Node ${data.node} fired: ${JSON.stringify(data.data)}`;
                document.getElementById('graph').prepend(div);
            }
        };
        fetch('/status').then(r => r.json()).then(d => {
            document.getElementById('health').innerHTML = `<span class="status-light green"></span> All systems nominal`;
        });
        fetch('/decisions').then(r => r.json()).then(d => {
            document.getElementById('decisions').innerHTML = d.map(dec => `<div>${dec.reasoning}</div>`).join('');
        });
        fetch('/workflows').then(r => r.json()).then(wfs => {
            document.getElementById('triggers').innerHTML = wfs.map(w => `<button onclick="trigger('${w.id}')">Trigger ${w.name}</button>`).join('');
        });
        async function trigger(id) {
            await fetch(`/workflows/${id}/trigger`, { method: 'POST' });
            document.getElementById('graph').prepend('<div class="node">Manual trigger queued: ' + id + '</div>');
        }
    </script>
</body>
</html>"#;

pub type BroadcastSender = broadcast::Sender<Value>;
pub type BroadcastReceiver = broadcast::Receiver<Value>;

pub async fn dashboard_handler() -> impl IntoResponse {
    Html(DASHBOARD_HTML.to_string())
}

pub async fn ws_handler(upgrade: WebSocketUpgrade) -> impl IntoResponse {
    upgrade.on_upgrade(|socket| handle_ws(socket))
}

async fn handle_ws(mut socket: WebSocket) {
    while let Some(Ok(msg)) = socket.recv().await {
        if let Ok(text) = msg.into_text() {
            if text == "ping" {
                let _ = socket.send(axum::extract::ws::Message::Text("pong".into())).await;
            }
        }
    }
}

pub async fn status_handler() -> impl IntoResponse {
    Json(serde_json::json!({
        "cluster": "healthy",
        "services": {"koboldcpp": "green", "watchdog": "green", "cesarops": "green"}
    }))
}

pub async fn workflows_handler() -> impl IntoResponse {
    Json(serde_json::json!([
        {"id": "wf-1", "name": "Self-Annealing Loop", "state": "idle"},
        {"id": "wf-2", "name": "Model Monitor", "state": "idle"}
    ]))
}

pub async fn trigger_handler(axum::extract::Path(id): axum::extract::Path<String>) -> impl IntoResponse {
    Json(serde_json::json!({"triggered": id, "status": "queued"}))
}

pub async fn decisions_handler() -> impl IntoResponse {
    Json(serde_json::json!([
        {"reasoning": "OOM detected, swapping to 14B model", "timestamp": "2024-01-01T00:00:00Z"}
    ]))
}
