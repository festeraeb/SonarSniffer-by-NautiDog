"""Symlink downloads dirs on Xeon and relaunch scan."""
import paramiko, time

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect('10.0.0.129', username='cesarops1', password='cesarops1', timeout=10)

def run(cmd, timeout=60):
    _, o, e = c.exec_command(cmd, timeout=timeout)
    try:
        out = o.read().decode(errors='replace').strip()
        err = e.read().decode(errors='replace').strip()
    except Exception:
        out, err = '', ''
    return out or err or '(ok)'

# Kill any lingering old scan
run('pkill -f lake_michigan_scan 2>/dev/null || true', 5)

# Verify TIF count at real path
count = run('find /home/cesarops1/downloads -name "*.tif" | wc -l')
print(f'[count] {count} TIF files at /home/cesarops1/downloads')

# Clear old log and launch fresh scan with data dir set to real path
# (rglob doesn't follow symlinks, so point directly at the real downloads dir)
run('rm -f ~/scan_log_xeon_now.txt')
scan_cmd = (
    'cd ~/wreckhunter2000-1 && '
    'CESAROPS_DATA_DIR=/home/cesarops1/downloads '
    'nohup .venv/bin/python lake_michigan_scan.py '
    '> ~/scan_log_xeon_now.txt 2>&1 &'
)
run(scan_cmd, timeout=5)
time.sleep(4)

print('[ps]', run('pgrep -la python | grep lake_michigan || echo not_running'))
print('[log head]')
print(run('head -30 ~/scan_log_xeon_now.txt 2>/dev/null || echo no_log'))

c.close()
