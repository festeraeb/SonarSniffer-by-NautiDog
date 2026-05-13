"""Check status of running scans on i7 and local machine."""
import paramiko, subprocess
from pathlib import Path

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"

def run(ssh, cmd, timeout=15):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    return (o.read().decode(errors="replace").strip() or
            e.read().decode(errors="replace").strip())

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)

print("=== i7 SCAN STATUS ===")
ps = run(c, "pgrep -la python | grep -i lake || echo 'scan not running'")
print(f"Process: {ps}")

log = run(c, r"tail -20 ~/scan_*.log 2>/dev/null | head -25 || echo 'no log'")
print(f"Log tail:\n{log}")

kmz = run(c, "find ~/wreckhunter2000-1/outputs -name '*.kmz' 2>/dev/null | head -5 || echo 'no outputs yet'")
print(f"KMZ outputs: {kmz}")

print("\n=== LOCAL SCAN STATUS ===")
import os
stdout_file = Path(__file__).parent.parent / "outputs" / "scan_stdout.txt"
if stdout_file.exists():
    lines = stdout_file.read_text(encoding="utf-8", errors="replace").splitlines()
    print("\n".join(lines[-15:]))
else:
    print("No local scan log found")

c.close()
