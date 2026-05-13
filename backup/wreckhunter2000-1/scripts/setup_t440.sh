#!/bin/bash
set -e
mkdir -p /opt/cesarops /opt/cesarops/data/tiles
cp /home/cesarops/wreckhunter2000-1/target/release/sovereign-cloud /opt/cesarops/sovereign-cloud
chmod +x /opt/cesarops/sovereign-cloud

cat > /etc/systemd/system/sovereign-cloud.service << 'UNIT'
[Unit]
Description=CESARops sovereign-cloud node API (T440 dual-P100)
After=network.target

[Service]
ExecStart=/opt/cesarops/sovereign-cloud
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
Environment="API_BIND_ADDR=0.0.0.0"
Environment="I7_HOST=10.0.0.56"
Environment="I7_TAILSCALE=100.85.138.4"
Environment="T440_HOST=10.0.0.61"
Environment="XENON_HOST=10.0.0.129"
Environment="P1000_HOST=10.0.0.204"
Environment="P1000_TAILSCALE=100.105.77.74"
Environment="PI_HOST=10.0.0.226"
Environment="PI_TAILSCALE=100.127.66.32"
Environment="LAPTOP_HOST=10.0.0.69"
Environment="XBOX_HOST=10.0.0.100"

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable sovereign-cloud
systemctl restart sovereign-cloud
sleep 3
systemctl status sovereign-cloud --no-pager -l | head -20
curl -sf http://localhost:8765/health && echo "" && echo "sovereign-cloud is live"
