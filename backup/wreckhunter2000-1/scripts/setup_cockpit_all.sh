#!/bin/bash
# Setup Cockpit + GPU monitoring on a CESARops node
# Run on each server: bash scripts/setup_cockpit_all.sh
#
# After running, access at: https://<node-ip>:9090
# Login with your regular system credentials (cesarops/cesarops)
set -e

echo "═══════════════════════════════════════════════════════"
echo "  CESARops Cockpit Setup"
echo "  Node: $(hostname)"
echo "═══════════════════════════════════════════════════════"

# ── 1. Install Cockpit ────────────────────────────────────────────────────────
echo ""
echo "[1/4] Installing Cockpit..."
sudo apt-get update -qq
sudo apt-get install -y cockpit cockpit-pcp cockpit-storaged cockpit-networkmanager
sudo systemctl enable --now cockpit.socket
echo "  ✓ Cockpit listening on :9090"

# ── 2. Install GPU monitoring tools ──────────────────────────────────────────
echo ""
echo "[2/4] Installing GPU monitoring..."
sudo apt-get install -y nvtop
echo "  ✓ nvtop installed"

# ── 3. GPU metrics cron (feeds into Cockpit PCP) ─────────────────────────────
echo ""
echo "[3/4] Setting up GPU metrics logging..."

sudo tee /usr/local/bin/cesarops-gpu-metrics.sh > /dev/null << 'SCRIPT'
#!/bin/bash
# Log GPU metrics to a JSON file readable by the frontend/Cockpit
METRICS_FILE="/var/lib/cesarops/gpu_metrics.json"
mkdir -p /var/lib/cesarops

if command -v nvidia-smi &>/dev/null; then
    nvidia-smi --query-gpu=index,name,temperature.gpu,utilization.gpu,utilization.memory,memory.used,memory.total,power.draw,fan.speed \
        --format=csv,noheader,nounits 2>/dev/null | python3 -c "
import sys, json
from datetime import datetime, timezone

gpus = []
for line in sys.stdin:
    parts = [p.strip() for p in line.strip().split(',')]
    if len(parts) >= 7:
        gpus.append({
            'index': int(parts[0]),
            'name': parts[1],
            'temp_c': int(parts[2]) if parts[2] != '[N/A]' else None,
            'gpu_util_pct': int(parts[3]) if parts[3] != '[N/A]' else None,
            'mem_util_pct': int(parts[4]) if parts[4] != '[N/A]' else None,
            'mem_used_mb': int(parts[5]) if parts[5] != '[N/A]' else None,
            'mem_total_mb': int(parts[6]) if parts[6] != '[N/A]' else None,
            'power_w': float(parts[7]) if len(parts) > 7 and parts[7] != '[N/A]' else None,
            'fan_pct': int(parts[8]) if len(parts) > 8 and parts[8] != '[N/A]' else None,
        })

data = {
    'timestamp': datetime.now(timezone.utc).isoformat(),
    'hostname': '$(hostname)',
    'gpu_count': len(gpus),
    'gpus': gpus,
}
print(json.dumps(data, indent=2))
" > "$METRICS_FILE"
else
    echo '{"timestamp":"'$(date -u +%FT%TZ)'","hostname":"'$(hostname)'","gpu_count":0,"gpus":[]}' > "$METRICS_FILE"
fi
SCRIPT

sudo chmod +x /usr/local/bin/cesarops-gpu-metrics.sh
sudo /usr/local/bin/cesarops-gpu-metrics.sh

# Run every 30 seconds via systemd timer (more reliable than cron for sub-minute)
sudo tee /etc/systemd/system/cesarops-gpu-metrics.service > /dev/null << 'SVC'
[Unit]
Description=CESARops GPU Metrics Collector

[Service]
Type=oneshot
ExecStart=/usr/local/bin/cesarops-gpu-metrics.sh
SVC

sudo tee /etc/systemd/system/cesarops-gpu-metrics.timer > /dev/null << 'TIMER'
[Unit]
Description=CESARops GPU Metrics Timer (every 30s)

[Timer]
OnBootSec=10
OnUnitActiveSec=30

[Install]
WantedBy=timers.target
TIMER

sudo systemctl daemon-reload
sudo systemctl enable --now cesarops-gpu-metrics.timer
echo "  ✓ GPU metrics logging every 30s to /var/lib/cesarops/gpu_metrics.json"

# ── 4. Allow Cockpit through firewall (if ufw is active) ─────────────────────
echo ""
echo "[4/4] Firewall..."
if command -v ufw &>/dev/null && sudo ufw status | grep -q "active"; then
    sudo ufw allow 9090/tcp
    echo "  ✓ Port 9090 allowed through ufw"
else
    echo "  ✓ No active firewall (or not ufw)"
fi

# ── Done ──────────────────────────────────────────────────────────────────────
echo ""
echo "═══════════════════════════════════════════════════════"
echo "  ✓ Cockpit ready!"
echo ""
echo "  Access: https://$(hostname -I | awk '{print $1}'):9090"
echo "  Login:  cesarops / (your password)"
echo ""
echo "  GPU metrics: cat /var/lib/cesarops/gpu_metrics.json"
echo "  Live GPU:    nvtop"
echo "═══════════════════════════════════════════════════════"
