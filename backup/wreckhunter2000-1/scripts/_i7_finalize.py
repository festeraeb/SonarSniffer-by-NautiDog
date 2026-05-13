"""
Write ~/.env on i7 with GITHUB_PAT + CESAROPS_DIR,
and upload the updated node_update.sh so future deploys work.
"""
import paramiko
from pathlib import Path

HOST, USER, PASS = "10.0.0.56", "cesarops", "cesarops"

def load_env(p):
    env = {}
    try:
        for line in Path(p).read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    except Exception:
        pass
    return env

local_env = load_env(Path(__file__).parent.parent / ".env")
PAT = local_env.get("GITHUB_PAT", "")

def run(ssh, cmd, timeout=60):
    _, o, e = ssh.exec_command(cmd, timeout=timeout)
    out = o.read().decode(errors="replace").strip()
    err = e.read().decode(errors="replace").strip()
    print(f"  {out or err or '(ok)'}")
    return out or err

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected to {HOST}")

# Write ~/.env with all keys needed by node_update.sh
home_env = f"""# ~/.env — auto-written by _i7_finalize.py
GITHUB_PAT={PAT}
CESAROPS_DIR=/home/{USER}/wreckhunter2000-1
CESAROPS_BRANCH=wreckhuntertools
"""
sftp = c.open_sftp()
with sftp.open(f"/home/{USER}/.env", "w") as f:
    f.write(home_env)
run(c, f"chmod 600 /home/{USER}/.env")
print("[+] ~/.env written")

# Upload updated node_update.sh
local_script = Path(__file__).parent / "node_update.sh"
remote_script = f"/home/{USER}/wreckhunter2000-1/scripts/node_update.sh"
sftp.put(str(local_script), remote_script)
run(c, f"chmod +x {remote_script}")
print("[+] node_update.sh updated on i7")
sftp.close()

# Test it
print("\n[+] Testing git pull via node_update.sh (dry-run) ...")
run(c, f"bash {remote_script} --dry-run 2>&1 | head -20", timeout=15)

c.close()
print("\n[DONE] i7 finalized. Future deploys:")
print("  .\\scripts\\deploy_to_pi.ps1 -Nodes pi,i7")
