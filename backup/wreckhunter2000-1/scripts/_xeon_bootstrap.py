"""One-shot Xeon bootstrap: add laptop SSH key, install Tailscale, check GPU."""
import paramiko, subprocess, sys

HOST = "10.0.0.162"
USER = "cesarops1"
PASS = "cesarops1"
LAPTOP_PUBKEY = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPEh2RlCcwUr/lYq+tSdoVl1x9hEi3843Vq2s+y55wYA wreckhunter-deploy"

def run(ssh, cmd, timeout=60):
    _, stdout, stderr = ssh.exec_command(cmd, timeout=timeout)
    out = stdout.read().decode(errors="replace").strip()
    err = stderr.read().decode(errors="replace").strip()
    return out, err

client = paramiko.SSHClient()
client.set_missing_host_key_policy(paramiko.AutoAddPolicy())
print(f"Connecting to {HOST} ...")
client.connect(HOST, username=USER, password=PASS, timeout=10)
print(f"[+] Connected to {HOST}")

# 1. Authorise laptop pubkey
out, _ = run(client,
    f"mkdir -p ~/.ssh && chmod 700 ~/.ssh && "
    f"grep -qF '{LAPTOP_PUBKEY}' ~/.ssh/authorized_keys 2>/dev/null || "
    f"echo '{LAPTOP_PUBKEY}' >> ~/.ssh/authorized_keys && "
    f"chmod 600 ~/.ssh/authorized_keys && echo key_ok"
)
print(f"[+] authorized_keys: {out}")

# 2. System info
out, _ = run(client, "lsb_release -d 2>/dev/null; uname -r; lspci | grep -i 'nvidia\\|vga' | head -5")
print(f"[+] System:\n{out}")

# 3. Resources
out, _ = run(client, "df -h / && free -h")
print(f"[+] Disk/RAM:\n{out}")

# 4. Tailscale
out, _ = run(client, "which tailscale 2>/dev/null || echo NOT_FOUND")
if "NOT_FOUND" in out:
    print("[~] Installing Tailscale ...")
    install_cmd = (
        "export DEBIAN_FRONTEND=noninteractive && "
        "curl -fsSL https://tailscale.com/install.sh | sh"
    )
    out2, err2 = run(client, install_cmd, timeout=120)
    print(f"    install stdout: {out2[-300:] if out2 else '(none)'}")
    if err2: print(f"    install stderr: {err2[-200:]}")
else:
    ts_state, _ = run(client,
        "tailscale status 2>&1 | head -3"
    )
    print(f"[+] Tailscale: {ts_state}")

# 5. Report tailscale auth URL if not authenticated
ts_up, _ = run(client, "tailscale status 2>&1 | head -1")
print(f"[+] Tailscale state: {ts_up}")
if "Logged out" in ts_up or "not running" in ts_up.lower() or "NOT_FOUND" in ts_up:
    print("[~] Tailscale not connected — run on Xeon:")
    print("      sudo tailscale up")

# 6. Enable and start SSH password auth (in case it needs it for future scripts)
out, _ = run(client,
    "sudo sed -i 's/^#*PasswordAuthentication.*/PasswordAuthentication yes/' /etc/ssh/sshd_config 2>/dev/null && "
    "sudo systemctl reload ssh 2>/dev/null && echo sshd_ok || echo sshd_skip"
)
print(f"[+] SSHD config: {out}")

client.close()
print("\n[DONE] Xeon bootstrapped. You can now:")
print(f"  ssh -i ~/.ssh/id_ed25519 {USER}@{HOST}")
