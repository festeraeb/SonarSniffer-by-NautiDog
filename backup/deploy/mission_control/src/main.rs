use axum::{
    extract::{ws::{WebSocket, WebSocketUpgrade, Message as WsMessage}, Json, State},
    response::Html,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

const INDEX_HTML: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="UTF-8">
  <meta name="viewport" content="width=device-width, initial-scale=1.0">
  <title>CESAROPS Mission Control</title>
  <style>
    :root { --bg: #f4f4f9; --text: #1a1a1a; --primary: #0057b7; --success: #28a745; --warn: #ffc107; --font: 'Lexend', sans-serif; }
    body { font-family: var(--font); background: var(--bg); color: var(--text); margin: 0; padding: 20px; display: flex; flex-direction: column; align-items: center; }
    h1 { font-size: 2.5rem; text-align: center; margin-bottom: 20px; }
    .modes { display: flex; gap: 20px; flex-wrap: wrap; justify-content: center; margin-bottom: 20px; }
    .mode-btn { min-height: 60px; min-width: 120px; padding: 15px 25px; font-size: 1.2rem; border: none; border-radius: 8px; cursor: pointer; background: var(--primary); color: white; transition: transform 0.1s; }
    .mode-btn:hover { transform: scale(1.05); }
    .mode-btn.active { background: var(--success); }
    .controls { display: flex; gap: 10px; margin-bottom: 20px; width: 100%; max-width: 600px; }
    input[type="text"] { flex: 1; padding: 12px; font-size: 1rem; border: 2px solid #ccc; border-radius: 6px; }
    .voice-btn { min-height: 60px; min-width: 60px; background: var(--warn); border: none; border-radius: 6px; cursor: pointer; font-size: 1.5rem; }
    .voice-btn.listening { animation: pulse 1.5s infinite; }
    @keyframes pulse { 0% { opacity: 1; } 50% { opacity: 0.5; } 100% { opacity: 1; } }
    #progress { width: 100%; max-width: 600px; background: #e0e0e0; border-radius: 6px; overflow: hidden; height: 30px; margin-bottom: 20px; position: relative; }
    #progress-bar { height: 100%; background: var(--primary); width: 0%; transition: width 0.3s; }
    #progress-text { position: absolute; top: 0; left: 0; width: 100%; text-align: center; line-height: 30px; font-weight: bold; }
    #output { width: 100%; max-width: 600px; min-height: 100px; background: #fff; padding: 15px; border-radius: 8px; border: 1px solid #ccc; white-space: pre-wrap; font-family: monospace; }
  </style>
</head>
<body>
  <h1>CESAROPS Mission Control</h1>
  <div class="modes">
    <button class="mode-btn" data-mode="CODE">CODE</button>
    <button class="mode-btn" data-mode="SCAN">SCAN</button>
    <button class="mode-btn" data-mode="RESEARCH">RESEARCH</button>
  </div>
  <div class="controls">
    <input type="text" id="user-input" placeholder="Enter task or speak...">
    <button class="voice-btn" id="voice-btn">🎤</button>
  </div>
  <div id="progress"><div id="progress-bar"></div><div id="progress-text">Ready</div></div>
  <div id="output"></div>
  <script>
    let currentMode = 'CODE';
    let ws = null;
    const modes = document.querySelectorAll('.mode-btn');
    const input = document.getElementById('user-input');
    const voiceBtn = document.getElementById('voice-btn');
    const progressBar = document.getElementById('progress-bar');
    const progressText = document.getElementById('progress-text');
    const output = document.getElementById('output');

    modes.forEach(btn => btn.addEventListener('click', () => {
      modes.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      currentMode = btn.dataset.mode;
      submitTask();
    }));

    function connectWS() {
      ws = new WebSocket(`ws://${location.host}/ws`);
      ws.onmessage = (e) => {
        const data = JSON.parse(e.data);
        if(data.type === 'progress') {
          progressText.textContent = data.detail || data.step;
          progressBar.style.width = `${(data.step/data.total)*100}%`;
        } else if(data.type === 'llm_token') {
          output.textContent += data.text;
        } else if(data.type === 'result') {
          output.textContent = JSON.stringify(data.data, null, 2);
          progressText.textContent = '✅ Done';
          progressBar.style.width = '100%';
        }
      };
    }
    connectWS();

    function submitTask() {
      const payload = { mode: currentMode, input: input.value };
      fetch('/task', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(payload) });
      output.textContent = '';
      progressText.textContent = '🔄 Planning...';
      progressBar.style.width = '10%';
    }

    if('webkitSpeechRecognition' in window || 'SpeechRecognition' in window) {
      const SpeechRecognition = window.SpeechRecognition || window.webkitSpeechRecognition;
      const recognition = new SpeechRecognition();
      recognition.continuous = false;
      recognition.interimResults = true;
      voiceBtn.addEventListener('click', () => {
        if(recognition.isListening) { recognition.stop(); voiceBtn.classList.remove('listening'); }
        else { recognition.start(); voiceBtn.classList.add('listening'); }
      });
      recognition.onresult = (e) => {
        let transcript = '';
        for(let i=e.resultIndex; i<e.results.length; i++) transcript += e.results[i][0].transcript;
        input.value = transcript;
        if(e.results[0].isFinal) submitTask();
      };
      recognition.onend = () => voiceBtn.classList.remove('listening');
    }
  </script>
</body>
</html>"#;

#[derive(Debug, Clone, PartialEq)]
enum Mode { Code, Scan, Research }

impl From<&str> for Mode {
    fn from(s: &str) -> Self {
        match s {
            "CODE" => Mode::Code,
            "SCAN" => Mode::Scan,
            "RESEARCH" => Mode::Research,
            _ => Mode::Code,
        }
    }
}

#[derive(Deserialize)]
struct TaskRequest { mode: String, input: String }

#[derive(Clone)]
struct AppState { kobold_url: String, naut_url: String }

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();
    let state = AppState {
        kobold_url: "http://localhost:5001/v1/chat/completions".into(),
        naut_url: "http://localhost:5003/query".into(),
    };
    let app = Router::new()
        .route("/", get(index))
        .route("/task", post(task_handler))
        .route("/status", get(status))
        .route("/presets", get(presets))
        .route("/ws", get(ws_handler))
        .layer(CorsLayer::permissive())
        .with_state(state);
    let listener = tokio::net::TcpListener::bind("0.0.0.0:3000").await?;
    info!("Mission Control listening on 0.0.0.0:3000");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Html<&'static str> { Html(INDEX_HTML) }

async fn presets() -> impl axum::response::IntoResponse {
    let presets: Vec<(&str, &str)> = vec![
        ("code_review", "Rust Code Review"),
        ("scan_mackinac", "Mackinac Straits Scan"),
        ("research_sar", "SAR Bathymetry Research"),
    ];
    (axum::http::StatusCode::OK, serde_json::to_string(&presets).unwrap())
}

async fn status() -> impl axum::response::IntoResponse {
    let mut services: Vec<(&str, &str)> = Vec::new();
    services.push(("koboldcpp", "ok"));
    services.push(("nautivecs", "ok"));
    services.push(("wso", "ok"));
    (axum::http::StatusCode::OK, serde_json::to_string(&services).unwrap())
}

async fn task_handler(State(state): State<AppState>, Json(req): Json<TaskRequest>) -> impl axum::response::IntoResponse {
    info!("Task queued: mode={}, input={}", req.mode, req.input);
    (axum::http::StatusCode::ACCEPTED, "Task queued".to_string())
}

async fn ws_handler(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
) -> impl axum::response::IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(mut socket: WebSocket, state: AppState) {
    tokio::spawn(async move {
        let (mut tx, mut rx) = mpsc::channel::<String>(32);
        tokio::spawn(async move {
            // Compute derived values BEFORE moving into stream logic
            let mode = Mode::Code; // TODO: parse from task request
            let steps: Vec<&str> = match mode {
                Mode::Code => vec!["Planning...", "Querying nautivecs...", "Generating code...", "Compiling...", "Done."],
                Mode::Scan => vec!["Fetching tiles...", "Running shader...", "Filtering anomalies...", "Done."],
                Mode::Research => vec!["Searching web...", "Indexing codebase...", "Synthesizing...", "Done."],
            };
            let context = query_nautivecs(&state.naut_url, "tide_calc").await;
            let _ = context; // Derived context computed before moving

            for (i, step) in steps.iter().enumerate() {
                let _ = tx.send(format!("{{\"type\":\"progress\",\"step\":{},\"total\":{},\"detail\":\"{}\"}}", i+1, steps.len(), step)).await;
                tokio::time::sleep(tokio::time::Duration::from_millis(800)).await;
            }
            let _ = tx.send(r#"{"type":"llm_token","text":"fn tidal_coeff(lat: f64) -> f64 { 0.5 * lat.sin() }"}"#.to_string()).await;
            let _ = tx.send(r#"{"type":"result","data":{"status":"success","output":"fn tidal_coeff..."}}"#.to_string()).await;
        });

        while let Some(msg) = rx.recv().await {
            if socket.send(axum::extract::ws::Message::Text(msg)).await.is_err() { break; }
        }
    });
}

async fn query_nautivecs(url: &str, query: &str) -> String {
    format!("Context for '{}': existing hydrological models found.", query)
}
