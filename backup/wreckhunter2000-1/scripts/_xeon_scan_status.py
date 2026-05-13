"""Quick status check for the Xeon (cesarops2) scan."""
import paramiko

HOST, USER, PASS = "10.0.0.129", "cesarops1", "cesarops1"

def run(ssh, cmd, timeout=15):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    return out or err

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)

print(f"=== Xeon (cesarops2 / {HOST}) ===")
print(f"GPU:     {run(c, 'nvidia-smi --query-gpu=name,memory.used,memory.total,utilization.gpu --format=csv,noheader')}")
print(f"Process: {run(c, 'pgrep -la python 2>/dev/null || echo none')}")
tifs = run(c, "find ~/downloads -name '*.tif' 2>/dev/null | wc -l")
print(f"TIFs:    {tifs} files")
log = run(c, "tail -20 ~/scan_log_xeon_*.txt 2>/dev/null || echo 'no log yet'")
print(f"Log tail:\n{log}")

c.close()
