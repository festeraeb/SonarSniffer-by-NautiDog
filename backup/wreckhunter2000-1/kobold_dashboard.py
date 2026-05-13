#!/usr/bin/env python3
"""
KOBOLDCPP DEPLOYMENT DASHBOARD
Flask web app for managing multi-machine KoboldCPP deployments

Shows status and allows control from web UI

Usage:
    python3 kobold_dashboard.py --port 8080
    
    Then visit: http://localhost:8080
"""

from flask import Flask, render_template_string, jsonify, request
import subprocess
import json
import threading
import time
from datetime import datetime

app = Flask(__name__)

# Machine config (mirror of deploy_kobold_multi.py)
MACHINES = {
    't440_dual_p100': {
        'name': 't440-dual-p100',
        'host': '10.0.0.61',
        'user': 'cesarops',
        'alt_user': 'cesarios',
        'gpu_type': 'Tesla P100 x2',
        'memory_gb': 32,
        'kobold_port': 5001,
        'priority': 1,
        'default_gpu_ids': '0,1',
    },
    'xeon': {
        'name': 'Xeon',
        'host': '100.102.158.111',
        'user': 'cesarops1',
        'gpu_type': 'GTX 1070',
        'memory_gb': 8,
        'kobold_port': 5001,
        'priority': 2,
        'default_gpu_ids': '0',
    },
    'p1000': {
        'name': 'p1000',
        'host': '100.105.77.74',
        'user': 'cesarops',
        'gpu_type': 'NVIDIA P106',
        'memory_gb': 3,
        'kobold_port': 5002,
        'priority': 3,
        'default_gpu_ids': '0',
    },
    'gtx1060': {
        'name': 'gtx1060',
        'host': '10.0.0.204',
        'user': 'cesarops',
        'gpu_type': 'GTX 1060',
        'memory_gb': 6,
        'kobold_port': 5001,
        'priority': 4,
        'default_gpu_ids': '0',
    }
}

MODEL_PROFILES = {
    'qwen35-a3b-q8': {
        'label': 'Qwen3.5-35B-A3B Q8 (recommended dual P100)',
        'model_path': '/mnt/garmour/models/Qwen3.5-35B-A3B-Instruct-Q8_0.gguf',
        'contextsize': 65536,
    },
    'qwen25-coder-32b-q4': {
        'label': 'Qwen2.5-Coder-32B Q4_K_M',
        'model_path': '/mnt/garmour/models/Qwen2.5-Coder-32B-Instruct-Q4_K_M.gguf',
        'contextsize': 65536,
    },
    'deepseek-r1-distill-qwen-7b-q8': {
        'label': 'DeepSeek-R1-Distill-Qwen-7B Q8_0',
        'model_path': '/mnt/garmour/models/DeepSeek-R1-Distill-Qwen-7B-Q8_0.gguf',
        'contextsize': 65536,
    },
}

def ssh_run(machine_id, cmd, timeout=10):
    """Execute SSH command"""
    try:
        m = MACHINES[machine_id]
        users = [m["user"]]
        if m.get("alt_user"):
            users.append(m["alt_user"])

        last_err = ""
        for user in users:
            full_cmd = f'ssh -o ConnectTimeout=5 {user}@{m["host"]} "{cmd}"'
            result = subprocess.run(full_cmd, shell=True, capture_output=True, text=True, timeout=timeout)
            if result.returncode == 0:
                return True, result.stdout.strip(), result.stderr.strip()
            last_err = result.stderr.strip() or result.stdout.strip() or f"ssh failed for {user}@{m['host']}"
        return False, "", last_err
    except Exception as e:
        return False, "", str(e)

def get_status(machine_id):
    """Get machine status"""
    if machine_id not in MACHINES:
        return None
    
    m = MACHINES[machine_id]
    
    # Check GPU
    success, gpu_out, _ = ssh_run(machine_id, "nvidia-smi -L")
    gpu_count = len([l for l in gpu_out.split('\n') if l.strip()]) if success else 0
    
    # Check KoboldCPP
    success, api_out, _ = ssh_run(machine_id, f"curl -s http://localhost:{m['kobold_port']}/api/v1/models 2>/dev/null | head -c 50")
    running = success and len(api_out) > 10 and 'error' not in api_out.lower()
    
    return {
        'id': machine_id,
        'name': m['name'],
        'host': m['host'],
        'priority': m.get('priority', 100),
        'gpu_type': m['gpu_type'],
        'memory_gb': m['memory_gb'],
        'gpu_count': gpu_count,
        'gpu_available': gpu_count > 0,
        'kobold_running': running,
        'kobold_port': m['kobold_port'],
        'last_update': datetime.now().isoformat()
    }

@app.route('/')
def dashboard():
    """Main dashboard page"""
    html = """
    <!DOCTYPE html>
    <html>
    <head>
        <title>KoboldCPP Multi-Machine Deployment</title>
        <meta charset="utf-8">
        <meta name="viewport" content="width=device-width, initial-scale=1">
        <style>
            * { margin: 0; padding: 0; box-sizing: border-box; }
            body { font-family: 'Segoe UI', sans-serif; background: #1e1e1e; color: #e0e0e0; padding: 20px; }
            .container { max-width: 1200px; margin: 0 auto; }
            h1 { margin-bottom: 30px; color: #64b5f6; }
            
            .machines-grid {
                display: grid;
                grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
                gap: 20px;
                margin-bottom: 30px;
            }
            
            .machine-card {
                background: #2d2d2d;
                border: 1px solid #444;
                border-radius: 8px;
                padding: 20px;
                transition: all 0.3s ease;
            }
            
            .machine-card:hover { border-color: #64b5f6; box-shadow: 0 0 10px rgba(100,181,246,0.2); }
            
            .machine-header {
                display: flex;
                justify-content: space-between;
                align-items: center;
                margin-bottom: 15px;
                border-bottom: 1px solid #444;
                padding-bottom: 10px;
            }
            
            .machine-name { font-size: 20px; font-weight: bold; color: #64b5f6; }
            .machine-host { font-size: 12px; color: #999; }
            
            .status-badge {
                display: inline-block;
                padding: 4px 12px;
                border-radius: 4px;
                font-size: 12px;
                font-weight: bold;
            }
            
            .status-ok { background: #4caf50; color: white; }
            .status-error { background: #f44336; color: white; }
            .status-pending { background: #ff9800; color: white; }
            
            .machine-stats {
                display: grid;
                grid-template-columns: 1fr 1fr;
                gap: 10px;
                margin: 15px 0;
            }
            
            .stat {
                background: #1e1e1e;
                padding: 10px;
                border-radius: 4px;
                border-left: 3px solid #64b5f6;
            }
            
            .stat-label { font-size: 12px; color: #999; }
            .stat-value { font-size: 16px; font-weight: bold; color: #64b5f6; }
            
            .gpu-status {
                margin: 15px 0;
                padding: 10px;
                background: #1e1e1e;
                border-radius: 4px;
            }
            
            .gpu-status-icon {
                display: inline-block;
                width: 12px;
                height: 12px;
                border-radius: 50%;
                margin-right: 8px;
                vertical-align: middle;
            }
            
            .gpu-ok { background: #4caf50; }
            .gpu-error { background: #f44336; }
            
            .kobold-status {
                margin: 15px 0;
                padding: 10px;
                background: #1e1e1e;
                border-radius: 4px;
            }
            
            .controls {
                display: flex;
                gap: 10px;
                margin-top: 15px;
            }
            
            button {
                flex: 1;
                padding: 10px;
                border: none;
                border-radius: 4px;
                font-weight: bold;
                cursor: pointer;
                transition: all 0.3s ease;
            }
            
            .btn-deploy { background: #4caf50; color: white; }
            .btn-deploy:hover { background: #45a049; }
            
            .btn-stop { background: #f44336; color: white; }
            .btn-stop:hover { background: #da190b; }
            
            .btn-restart { background: #ff9800; color: white; }
            .btn-restart:hover { background: #e68900; }
            
            .global-controls {
                background: #2d2d2d;
                border: 1px solid #444;
                border-radius: 8px;
                padding: 20px;
                margin-bottom: 20px;
                display: flex;
                flex-wrap: wrap;
                gap: 10px;
            }
            
            .global-controls button { flex: 1; max-width: 200px; }
            
            .loading { opacity: 0.5; pointer-events: none; }
            
            .message {
                padding: 15px;
                border-radius: 4px;
                margin-bottom: 20px;
                display: none;
            }
            
            .message.show { display: block; }
            .message.success { background: #4caf50; color: white; }
            .message.error { background: #f44336; color: white; }
        </style>
    </head>
    <body>
        <div class="container">
            <h1>🚀 KoboldCPP Multi-Machine Deployment</h1>
            
            <div id="message" class="message"></div>
            
            <div class="global-controls">
                <select id="profile-select" style="padding:10px;border-radius:4px;background:#1e1e1e;color:#e0e0e0;border:1px solid #444;"></select>
                <select id="gpu-mode-select" style="padding:10px;border-radius:4px;background:#1e1e1e;color:#e0e0e0;border:1px solid #444;">
                    <option value="both">Use Both GPUs (if available)</option>
                    <option value="one">Use One GPU</option>
                </select>
                <input id="gpu-ids-input" type="text" placeholder="GPU IDs e.g. 0 or 0,1" style="padding:10px;border-radius:4px;background:#1e1e1e;color:#e0e0e0;border:1px solid #444;min-width:180px;" title="Specify explicit GPU IDs for KoboldCPP on the target host" />
                <button class="btn-deploy" onclick="deployAll()">Deploy All</button>
                <button class="btn-restart" onclick="restartAll()">Restart All</button>
                <button class="btn-stop" onclick="stopAll()">Stop All</button>
                <button style="background:#2196F3; color:white;" onclick="refreshStatus()">🔄 Refresh Status</button>
            </div>
            
            <div class="machines-grid" id="machines-grid"></div>
        </div>
        
        <script>
            const MACHINES = {
                't440_dual_p100': { name: 't440-dual-p100', port: 5001, priority: 1 },
                'xeon': { name: 'Xeon', port: 5001, priority: 2 },
                'p1000': { name: 'p1000', port: 5002, priority: 3 },
                'gtx1060': { name: 'gtx1060', port: 5001, priority: 4 }
            };
            const PROFILES = {{ profiles_json|safe }};

            function initSelectors() {
                const profileSelect = document.getElementById('profile-select');
                profileSelect.innerHTML = '';
                for (const [key, p] of Object.entries(PROFILES)) {
                    const opt = document.createElement('option');
                    opt.value = key;
                    opt.textContent = p.label;
                    profileSelect.appendChild(opt);
                }
            }

            function selectedOptions() {
                const profile = document.getElementById('profile-select').value;
                const gpu_mode = document.getElementById('gpu-mode-select').value;
                const gpu_ids = document.getElementById('gpu-ids-input').value.trim();
                return { profile, gpu_mode, gpu_ids };
            }
            
            async function getStatus() {
                try {
                    const response = await fetch('/api/status');
                    return await response.json();
                } catch (e) {
                    console.error('Status fetch error:', e);
                    return {};
                }
            }
            
            async function executeAction(machine, action) {
                const { profile, gpu_mode, gpu_ids } = selectedOptions();
                showMessage(`${action.toUpperCase()} ${machine}...`, 'pending');
                try {
                    const response = await fetch(`/api/action`, {
                        method: 'POST',
                        headers: { 'Content-Type': 'application/json' },
                        body: JSON.stringify({ machine, action, profile, gpu_mode, gpu_ids })
                    });
                    const result = await response.json();
                    showMessage(result.message, result.success ? 'success' : 'error');
                    setTimeout(refreshStatus, 2000);
                } catch (e) {
                    showMessage(`Error: ${e}`, 'error');
                }
            }
            
            async function refreshStatus() {
                const status = await getStatus();
                const grid = document.getElementById('machines-grid');
                grid.innerHTML = '';
                
                const entries = Object.entries(status).sort((a, b) => (a[1].priority || 100) - (b[1].priority || 100));
                for (const [id, data] of entries) {
                    const card = document.createElement('div');
                    card.className = 'machine-card';
                    
                    const gpuClass = data.gpu_available ? 'status-ok' : 'status-error';
                    const koboldClass = data.kobold_running ? 'status-ok' : 'status-error';
                    const gpuIcon = data.gpu_available ? '✓' : '✗';
                    const koboldIcon = data.kobold_running ? '✓' : '✗';
                    
                    card.innerHTML = `
                        <div class="machine-header">
                            <div>
                                <div class="machine-name">${data.name}</div>
                                <div class="machine-host">${data.host}</div>
                                <div class="machine-host">Priority: ${data.priority ?? 100}</div>
                            </div>
                        </div>
                        
                        <div class="machine-stats">
                            <div class="stat">
                                <div class="stat-label">GPU Type</div>
                                <div class="stat-value">${data.gpu_type}</div>
                            </div>
                            <div class="stat">
                                <div class="stat-label">Memory</div>
                                <div class="stat-value">${data.memory_gb}GB</div>
                            </div>
                        </div>
                        
                        <div class="gpu-status">
                            <span class="gpu-status-icon ${data.gpu_available ? 'gpu-ok' : 'gpu-error'}"></span>
                            GPU: ${data.gpu_count > 0 ? 'Online' : 'Offline'}
                            <span class="status-badge ${gpuClass}">${gpuIcon}</span>
                        </div>
                        
                        <div class="kobold-status">
                            <span class="gpu-status-icon ${data.kobold_running ? 'gpu-ok' : 'gpu-error'}"></span>
                            KoboldCPP (port ${data.kobold_port}): ${data.kobold_running ? 'Running' : 'Stopped'}
                            <span class="status-badge ${koboldClass}">${koboldIcon}</span>
                        </div>
                        
                        <div class="controls">
                            <button class="btn-deploy" onclick="executeAction('${id}', 'deploy')">Deploy</button>
                            <button class="btn-restart" onclick="executeAction('${id}', 'restart')">Restart</button>
                            <button class="btn-stop" onclick="executeAction('${id}', 'stop')">Stop</button>
                        </div>
                    `;
                    grid.appendChild(card);
                }
            }
            
            async function deployAll() {
                if (confirm('Deploy KoboldCPP to ALL machines?')) {
                    await executeAction(Object.keys(MACHINES).join(','), 'deploy');
                }
            }
            
            async function restartAll() {
                if (confirm('Restart KoboldCPP on ALL machines?')) {
                    await executeAction(Object.keys(MACHINES).join(','), 'restart');
                }
            }
            
            async function stopAll() {
                if (confirm('Stop KoboldCPP on ALL machines?')) {
                    await executeAction(Object.keys(MACHINES).join(','), 'stop');
                }
            }
            
            function showMessage(text, type) {
                const msg = document.getElementById('message');
                msg.textContent = text;
                msg.className = `message show ${type}`;
                setTimeout(() => msg.classList.remove('show'), 5000);
            }
            
            // Initial load
            initSelectors();
            refreshStatus();
            setInterval(refreshStatus, 10000); // Auto-refresh every 10s
        </script>
    </body>
    </html>
    """
    return render_template_string(html, profiles_json=json.dumps(MODEL_PROFILES))

@app.route('/api/status')
def api_status():
    """API endpoint for status"""
    result = {}
    for machine_id in MACHINES.keys():
        result[machine_id] = get_status(machine_id)
    return jsonify(result)

@app.route('/api/action', methods=['POST'])
def api_action():
    """API endpoint for deployment actions"""
    data = request.get_json()
    machine = data.get('machine', '')
    action = data.get('action', 'status')
    profile = data.get('profile')
    gpu_mode = data.get('gpu_mode', 'both')
    gpu_ids = data.get('gpu_ids', None)
    
    # For multi-machine actions, split and execute
    if ',' in machine:
        for m in machine.split(','):
            execute_action_on_machine(m.strip(), action, profile=profile, gpu_mode=gpu_mode, gpu_ids=gpu_ids)
        return jsonify({'success': True, 'message': f'{action.upper()} queued on multiple machines'})
    else:
        success, msg = execute_action_on_machine(machine, action, profile=profile, gpu_mode=gpu_mode, gpu_ids=gpu_ids)
        return jsonify({'success': success, 'message': msg})

def execute_action_on_machine(machine_id, action, profile=None, gpu_mode='both', gpu_ids=None):
    """Execute action on a single machine"""
    if machine_id not in MACHINES:
        return False, f"Unknown machine: {machine_id}"

    m = MACHINES[machine_id]
    profile_data = MODEL_PROFILES.get(profile) if profile else None
    model_path = profile_data['model_path'] if profile_data else '/mnt/garmour/models/llama3-8b.gguf'
    contextsize = profile_data['contextsize'] if profile_data else 65536
    default_gpu_ids = m.get('default_gpu_ids', '0')
    if gpu_ids:
        gpu_ids = ",".join([g.strip() for g in str(gpu_ids).split(',') if g.strip()])
    else:
        gpu_ids = default_gpu_ids.split(',')[0] if gpu_mode == 'one' else default_gpu_ids

    if action == 'deploy':
        cmd = f"""
nohup python3 ~/launch_koboldcpp.py \\
    --model {model_path} \\
    --port {m['kobold_port']} \\
    --gpu-ids {gpu_ids} \\
    --contextsize {contextsize} \\
    > ~/koboldcpp.log 2>&1 &
"""
        success, out, err = ssh_run(machine_id, cmd)
        return success, f"Deployed to {m['name']} ({profile or 'default'}, {gpu_mode} GPU mode)"

    elif action == 'stop':
        success, out, err = ssh_run(machine_id, "pkill -f koboldcpp")
        return True, f"Stopped {m['name']}"

    elif action == 'restart':
        ssh_run(machine_id, "pkill -f koboldcpp")
        time.sleep(2)
        cmd = f"""
nohup python3 ~/launch_koboldcpp.py \\
    --model {model_path} \\
    --port {m['kobold_port']} \\
    --gpu-ids {gpu_ids} \\
    --contextsize {contextsize} \\
    > ~/koboldcpp.log 2>&1 &
"""
        success, out, err = ssh_run(machine_id, cmd)
        return success, f"Restarted {m['name']} ({profile or 'default'}, {gpu_mode} GPU mode)"

    return False, "Unknown action"

if __name__ == '__main__':
    import sys
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
    print(f"🌐 Dashboard running at http://localhost:{port}")
    app.run(debug=False, host='0.0.0.0', port=port)
