#!/usr/bin/env python3
"""
Update sovereign-cloud systemd service with LLM_BACKENDS env var.
"""

import paramiko
import os
from pathlib import Path

_dotenv = {}
if Path('.env').exists():
    for line in Path('.env').read_text().splitlines():
        if '=' in line and not line.startswith('#'):
            k, v = line.split('=', 1)
            _dotenv[k.strip()] = v.strip()

def ssh_connect(host, user, password):
    client = paramiko.SSHClient()
    client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
    client.connect(host, username=user, password=password, timeout=10)
    return client

def update_systemd():
    host = _dotenv.get('XENON_HOST', '10.0.0.129')
    user = _dotenv.get('XENON_USER', 'cesarops1')
    password = _dotenv.get('XENON_PASS', 'cesarops1')

    client = ssh_connect(host, user, password)

    # LLM_BACKENDS map
    backends = {
        "phi-mini": "http://10.0.0.56:5002/v1",  # i7 P1000
        "qwen-14b": "http://10.0.0.61:5003/v1",  # T440 P100
    }

    backends_json = str(backends).replace("'", '"')

    # Update systemd service
    cmd = f"""
echo '{password}' | sudo -S systemctl stop sovereign-cloud
sudo sed -i 's|Environment=.*LLM_BACKENDS.*|Environment=LLM_BACKENDS={backends_json}|g' /etc/systemd/system/sovereign-cloud.service
sudo sed -i '/Environment=LLM_BACKENDS/d' /etc/systemd/system/sovereign-cloud.service
sudo sh -c 'echo "Environment=LLM_BACKENDS={backends_json}" >> /etc/systemd/system/sovereign-cloud.service'
sudo systemctl daemon-reload
sudo systemctl start sovereign-cloud
sudo systemctl status sovereign-cloud --no-pager
"""

    stdin, stdout, stderr = client.exec_command(cmd)
    print("STDOUT:", stdout.read().decode())
    print("STDERR:", stderr.read().decode())

    client.close()

if __name__ == "__main__":
    update_systemd()