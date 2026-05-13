#!/usr/bin/env python3
"""
Deploy and launch KoboldCpp instances on i7 and T440.
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

def deploy_kobold():
    # i7
    host = _dotenv.get('I7_HOST', '10.0.0.56')
    user = _dotenv.get('I7_USER', 'cesarops')
    password = _dotenv.get('I7_PASS', 'cesarops')

    client = ssh_connect(host, user, password)

    # Copy launch script
    sftp = client.open_sftp()
    sftp.put('launch_kobold_i7.sh', '/home/cesarops/launch_kobold.sh')
    sftp.close()

    # Make executable and launch
    cmd = """
chmod +x /home/cesarops/launch_kobold.sh
pkill -f koboldcpp || true
nohup /home/cesarops/launch_kobold.sh > /home/cesarops/kobold_i7.log 2>&1 &
sleep 5
ps aux | grep koboldcpp
"""
    stdin, stdout, stderr = client.exec_command(cmd)
    print("i7 STDOUT:", stdout.read().decode())
    print("i7 STDERR:", stderr.read().decode())
    client.close()

    # T440
    host = _dotenv.get('T440_HOST', '10.0.0.61')
    user = _dotenv.get('T440_USER', 'cesarops')
    password = _dotenv.get('T440_PASS', 'cesarops')

    client = ssh_connect(host, user, password)

    sftp = client.open_sftp()
    sftp.put('launch_kobold_t440.sh', '/home/cesarops/launch_kobold.sh')
    sftp.close()

    cmd = """
chmod +x /home/cesarops/launch_kobold.sh
pkill -f koboldcpp || true
nohup /home/cesarops/launch_kobold.sh > /home/cesarops/kobold_t440.log 2>&1 &
sleep 5
ps aux | grep koboldcpp
"""
    stdin, stdout, stderr = client.exec_command(cmd)
    print("T440 STDOUT:", stdout.read().decode())
    print("T440 STDERR:", stderr.read().decode())
    client.close()

if __name__ == "__main__":
    deploy_kobold()