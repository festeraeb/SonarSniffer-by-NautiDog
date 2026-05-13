#!/usr/bin/env python3
"""Fix mission control build errors"""
import pathlib

p = pathlib.Path("/home/cesarops/wreckhunter2000-1/cesarops-mission-control/src/main.rs")
t = p.read_text()

# 1. Fix WebSocket import - need axum::extract::ws::WebSocket
if "use axum::" in t and "WebSocket" not in t.split("use")[0]:
    # Add WebSocket to imports
    t = t.replace(
        "use axum::",
        "use axum::extract::ws::{WebSocket, WebSocketUpgrade, Message as WsMessage};\nuse axum::",
        1
    )
    # If there's already a ws import, skip
    if t.count("WebSocket") > 3:
        # Already imported elsewhere, just ensure it's right
        pass

# 2. Fix Mode::from(&state.kobold_url) - this makes no sense, it should parse the mode from the task
# Replace with a placeholder that just returns Code mode
t = t.replace(
    "let mode = Mode::from(&state.kobold_url);",
    'let mode = Mode::Code; // TODO: parse from task request'
)

# 3. Fix tx.send() expecting String not &str
t = t.replace(
    '.send(r#"{"type":"llm_token"',
    '.send(r#"{"type":"llm_token"'
)
# Actually just add .to_string() to the send calls
import re
t = re.sub(
    r'tx\.send\((r#"[^"]*"#)\)',
    r'tx.send(\1.to_string())',
    t
)
# Handle multi-line r# strings
t = re.sub(
    r'let _ = tx\.send\(r#"(.+?)"#\)\.await',
    r'let _ = tx.send(r#"\1"#.to_string()).await',
    t
)

# 4. Check if Cargo.toml has axum ws feature
cargo_path = pathlib.Path("/home/cesarops/wreckhunter2000-1/cesarops-mission-control/Cargo.toml")
cargo = cargo_path.read_text()
if "ws" not in cargo:
    cargo = cargo.replace(
        'axum = "0.8"',
        'axum = { version = "0.8", features = ["ws"] }'
    )
    # If it was already a table format
    if 'axum = { version' not in cargo and 'axum =' in cargo:
        cargo = cargo.replace('axum = "0.8"', 'axum = { version = "0.8", features = ["ws"] }')
    cargo_path.write_text(cargo)
    print("Fixed Cargo.toml: added ws feature")

p.write_text(t)
print("Fixed main.rs: WebSocket import, Mode, send types")
