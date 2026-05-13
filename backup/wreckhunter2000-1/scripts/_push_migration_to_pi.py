import paramiko
import sqlite3, tempfile, os

PI = '100.127.66.32'
ssh = paramiko.SSHClient()
ssh.set_missing_host_key_policy(paramiko.AutoAddPolicy())
ssh.connect(PI, username='pi', timeout=15)

# Show Pi DB path from service file
_, out, _ = ssh.exec_command('grep -E "QUEUE_DB_PATH|ExecStart" /etc/systemd/system/wrecks-api.service 2>/dev/null || grep -E "QUEUE_DB_PATH|ExecStart" /home/pi/wreckhunter2000-1/wrecks_api/*.service 2>/dev/null || echo "not found"')
print("service config:", out.read().decode())

# List db/ dir
_, out, _ = ssh.exec_command('ls -la /home/pi/wreckhunter2000-1/db/')
print("db dir:", out.read().decode())

# Run migration directly on Pi
_, out, err = ssh.exec_command('python3 /home/pi/wreckhunter2000-1/scripts/migrate_add_job_type.py')
print("migration stdout:", out.read().decode())
print("migration stderr:", err.read().decode())

ssh.close()
