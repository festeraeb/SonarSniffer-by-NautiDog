#!/usr/bin/env python3
"""
Verify and fix Samba share on i7.
Run from: python scripts/_fix_samba_i7.py
"""
import subprocess, sys

I7 = "10.0.0.56"
USER = "cesarops"
PW   = "cesarops"

def ssh(cmd: str) -> str:
    result = subprocess.run(
        ["ssh", "-o", "StrictHostKeyChecking=no",
         "-o", "ConnectTimeout=10",
         f"{USER}@{I7}", cmd],
        capture_output=True, text=True
    )
    return result.stdout.strip() + result.stderr.strip()

print("=== Fixing Samba on i7 ===\n")

# 1. Check current smb.conf for our share
print("--- smb.conf cesarops-armor section ---")
print(ssh("grep -A10 'cesarops-armor' /etc/samba/smb.conf"))

# 2. Make sure samba user exists
print("\n--- Ensure samba user 'cesarops' exists ---")
print(ssh(f"echo -e '{PW}\n{PW}' | sudo smbpasswd -a {USER} 2>&1 || echo 'already exists'"))
print(ssh(f"sudo smbpasswd -e {USER} 2>&1"))  # enable

# 3. Restart nmbd + smbd cleanly
print("\n--- Restart nmbd + smbd ---")
print(ssh("sudo systemctl restart nmbd smbd 2>&1"))

import time; time.sleep(2)

# 4. testparm check
print("\n--- testparm ---")
print(ssh("sudo testparm -s 2>&1 | tail -30"))

# 5. Try listing shares
print("\n--- smbclient -L localhost ---")
out = ssh(f"smbclient -L localhost -U {USER}%{PW} 2>&1")
print(out)
if "cesarops-armor" in out:
    print("\n✓ Share is visible in smbclient")
else:
    print("\n⚠ Share still not listed — checking mount point...")
    print(ssh("ls -la /mnt/cesarops-armor/ 2>&1 | head -10"))
    # Try adding share manually if missing
    print("\n--- Checking if share block exists ---")
    check = ssh("grep -c 'cesarops-armor' /etc/samba/smb.conf")
    if check.strip() == "0":
        print("Share block MISSING — re-adding...")
        share_block = """\\n[cesarops-armor]\\n   path = /mnt/cesarops-armor\\n   browseable = yes\\n   read only = no\\n   guest ok = no\\n   valid users = cesarops\\n   force user = cesarops\\n"""
        print(ssh(f"echo -e '{share_block}' | sudo tee -a /etc/samba/smb.conf"))
        print(ssh("sudo systemctl reload smbd 2>&1"))
    else:
        print(f"Share block present ({check} lines). Mount may be missing:")
        print(ssh("mount | grep cesarops-armor"))
        print(ssh("sudo mount -a 2>&1"))
