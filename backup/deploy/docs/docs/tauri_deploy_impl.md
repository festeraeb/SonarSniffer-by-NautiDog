

Here are the two requested deliverables: the Tauri Desktop Wrapper and the Web Deployment files.

## 1. Tauri Desktop Wrapper (Windows)

This creates a standalone Tauri application that wraps the Mission Control UI. It handles the connection logic (LAN vs. Tunnel) internally via the `tauri.conf.json` configuration or can be hardcoded. For simplicity, we will configure it to point to the LAN IP by default, which you can change easily.

### === FILE: tauri-mission-control/package.json ===
```json
{
  "name": "cesarops-mission-control",
  "version": "1.0.0",
  "description": "CESAROPS Mission Control Desktop App",
  "author": "CESAROPS",
  "license": "MIT",
  "scripts": {
    "tauri": "tauri",
    "dev": "tauri dev",
    "build": "tauri build"
  },
  "dependencies": {},
  "devDependencies": {
    "@tauri-apps/cli": "^1.5.0"
  }
}
```

### === FILE: tauri-mission-control/src-tauri/tauri.conf.json ===
```json
{
  "build": {
    "beforeBuildCommand": "",
    "beforeDevCommand": "",
    "devUrl": "http://100.72.182.77:3000",
    "distDir": "../index.html",
    "withGlobalTauri": true
  },
  "package": {
    "productName": "CESAROPS Mission Control",
    "version": "1.0.0"
  },
  "tauri": {
    "allowlist": {
      "all": false,
      "shell": {
        "open": true
      }
    },
    "bundle": {
      "active": true,
      "category": "DeveloperTool",
      "copyright": "",
      "deb": {
        "depends": []
      },
      "externalBin": [],
      "icon": [],
      "identifier": "com.cesarops.missioncontrol",
      "linux": {
        "deb": {
          "depends": []
        }
      },
      "macOS": {
        "entitlements": null,
        "exceptionDomain": "",
        "frameworks": [],
        "providerShortName": null,
        "signingIdentity": null
      },
      "windows": {
        "certificateThumbprint": null,
        "digestAlgorithm": "sha256",
        "timestampUrl": ""
      }
    },
    "security": {
      "csp": null
    },
    "systemTray": {
      "iconPath": "icons/icon.png",
      "iconAsTemplate": true
    },
    "updater": {
      "active": false
    },
    "windows": [
      {
        "fullscreen": false,
        "height": 800,
        "resizable": true,
        "title": "CESAROPS Mission Control",
        "width": 1200,
        "url": "http://100.72.182.77:3000"
      }
    ]
  }
}
```

### === FILE: tauri-mission-control/src-tauri/src/main.rs ===
```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tauri::Builder::default()
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
```

### === FILE: tauri-mission-control/index.html ===
```html
<!DOCTYPE html>
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

    // Auto-detect host for WebSocket if running in Tauri
    // In Tauri, location.host will be the configured devUrl or build URL
    const wsHost = window.location.hostname || '100.72.182.77';
    const wsPort = window.location.port || '3000';

    modes.forEach(btn => btn.addEventListener('click', () => {
      modes.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      currentMode = btn.dataset.mode;
      submitTask();
    }));

    function connectWS() {
      // Use WSS if https, WS if http
      const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
      ws = new WebSocket(`${protocol}//${wsHost}:${wsPort}/ws`);
      
      ws.onopen = () => {
        progressText.textContent = '🟢 Connected';
      };

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

      ws.onerror = (err) => {
        progressText.textContent = '❌ Connection Error';
        console.error("WS Error", err);
      };
    }
    connectWS();

    function submitTask() {
      const payload = { mode: currentMode, input: input.value };
      fetch('/task', { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(payload) })
        .then(() => {
          output.textContent = '';
          progressText.textContent = '🔄 Planning...';
          progressBar.style.width = '10%';
        })
        .catch(err => {
          progressText.textContent = '❌ Task Failed';
          console.error("Task Error", err);
        });
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
</html>
```

### === FILE: tauri-mission-control/setup.ps1 ===
```powershell
# CESAROPS Mission Control Tauri Setup Script
# Run this in PowerShell from the tauri-mission-control directory

Write-Host "Installing Node.js dependencies..." -ForegroundColor Cyan
npm install

Write-Host "Installing Tauri CLI globally..." -ForegroundColor Cyan
npm install -g @tauri-apps/cli

Write-Host "Generating Tauri configuration..." -ForegroundColor Cyan
npx tauri init

Write-Host "Building the app..." -ForegroundColor Cyan
npx tauri build

Write-Host "Build complete! Check the target/release directory." -ForegroundColor Green
```

---

## 2. Web Deployment to IONOS

This places the standalone Mission Control HTML into the `dist-web` folder, ready for the existing deploy script. It configures the backend to point to the public tunnel (`api.cesarops.org`).

### === FILE: tauri/dist-web/mission-control/index.html ===
```html
<!DOCTYPE html>
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

    // Configuration for IONOS Deployment
    const API_HOST = 'api.cesarops.org';
    const API_PORT = '443'; // HTTPS
    const PROTOCOL = 'https:';

    modes.forEach(btn => btn.addEventListener('click', () => {
      modes.forEach(b => b.classList.remove('active'));
      btn.classList.add('active');
      currentMode = btn.dataset.mode;
      submitTask();
    }));

    function connectWS() {
      // Connect to WSS on the API host
      ws = new WebSocket(`wss://${API_HOST}/ws`);
      
      ws.onopen = () => {
        progressText.textContent = '🟢 Connected';
      };

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

      ws.onerror = (err) => {
        progressText.textContent = '❌ Connection Error';
        console.error("WS Error", err);
      };
    }
    connectWS();

    function submitTask() {
      const payload = { mode: currentMode, input: input.value };
      fetch(`https://${API_HOST}/task`, { method: 'POST', headers: {'Content-Type':'application/json'}, body: JSON.stringify(payload) })
        .then(() => {
          output.textContent = '';
          progressText.textContent = '🔄 Planning...';
          progressBar.style.width = '10%';
        })
        .catch(err => {
          progressText.textContent = '❌ Task Failed';
          console.error("Task Error", err);
        });
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
</html>
```

### === FILE: scripts/deploy_mission_control.ps1 ===
```powershell
# Deploy Mission Control to IONOS
# Run this from the root of the repository

Write-Host "Deploying Mission Control to IONOS..." -ForegroundColor Cyan

# Ensure the target directory exists
$targetDir = "tauri/dist-web/mission-control"
if (-not (Test-Path $targetDir)) {
    New-Item -ItemType Directory -Force -Path $targetDir | Out-Null
}

# Copy the standalone HTML file
Copy-Item -Path "tauri-mission-control/index.html" -Destination "$targetDir/index.html" -Force

Write-Host "File copied to $targetDir/index.html" -ForegroundColor Green

# Run the existing deploy script
Write-Host "Running deploy_web.py --ionos..." -ForegroundColor Yellow
python scripts/deploy_web.py --ionos

Write-Host "Deployment complete!" -ForegroundColor Green
```