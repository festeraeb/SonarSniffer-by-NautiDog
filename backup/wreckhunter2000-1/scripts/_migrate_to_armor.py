#!/usr/bin/env python3
"""
Migrate TIFs from ~/downloads → /mnt/cesarops-armor/downloads/ on i7.
Also copies the DB to ArmorATD if it exists in the repo.

Run: python scripts/_migrate_to_armor.py
"""
import os
import sys
import subprocess
import paramiko
from pathlib import Path

I7   = "10.0.0.56"
USER = "cesarops"
PW   = "cesarops"

ARMOR = "/mnt/cesarops-armor"
SRC_DOWNLOADS = "/home/cesarops/downloads"
DST_DOWNLOADS = f"{ARMOR}/downloads"
DB_NAME       = "LAKE_MICHIGAN_CENSUS_2026.db"

def ssh(client, cmd, timeout=60):
    _, out, err = client.exec_command(cmd, timeout=timeout)
    return (out.read() + err.read()).decode(errors="replace").strip()

print("=== Migrate TIFs to ArmorATD ===\n")

c = paramiko.SSHClient()
c.set_missing_host_key_policy(paramiko.AutoAddPolicy())
c.connect(I7, username=USER, password=PW, timeout=10)

# 1. Confirm armor is mounted
mount_check = ssh(c, f"mountpoint -q {ARMOR} && echo MOUNTED || echo NOT_MOUNTED")
print(f"[armor_mount] {mount_check}")
if mount_check != "MOUNTED":
    print("ERROR: ArmorATD is not mounted at", ARMOR)
    c.close()
    sys.exit(1)

# 2. Check free space
df = ssh(c, f"df -h {ARMOR}")
print(f"[space]\n{df}\n")

# 3. Check source size
du = ssh(c, f"du -sh {SRC_DOWNLOADS}/* 2>/dev/null")
print(f"[source sizes]\n{du}\n")

# 4. Create destination dirs
print(f"[mkdir] {DST_DOWNLOADS}")
print(ssh(c, f"mkdir -p {DST_DOWNLOADS}"))

# 5. Rsync TIFs
print(f"\n[rsync] {SRC_DOWNLOADS}/ → {DST_DOWNLOADS}/")
print("This may take a few minutes for the first sync...\n")
rsync_out = ssh(c, 
    f"rsync -av --progress --stats {SRC_DOWNLOADS}/ {DST_DOWNLOADS}/ 2>&1",
    timeout=600
)
# Show last 20 lines
lines = rsync_out.split("\n")
print("\n".join(lines[-20:]))

# 6. Verify count
src_count = ssh(c, f"find {SRC_DOWNLOADS} -name '*.tif' | wc -l")
dst_count = ssh(c, f"find {DST_DOWNLOADS} -name '*.tif' | wc -l")
print(f"\n[tif_count] src={src_count}  dst={dst_count}")

if src_count != dst_count:
    print("WARNING: counts differ — check rsync output above")

# 7. Copy DB if it exists on the Xeon/laptop
# Try to find it in the armor dir first
db_check = ssh(c, f"ls -lh {ARMOR}/{DB_NAME} 2>/dev/null || echo MISSING")
print(f"\n[db_on_armor] {db_check}")

if "MISSING" in db_check:
    # Check if it's in the repo dir (i7's repo)
    repo_db = f"/home/cesarops/wreckhunter2000-1/{DB_NAME}"
    repo_check = ssh(c, f"ls -lh {repo_db} 2>/dev/null || echo MISSING")
    if "MISSING" not in repo_check:
        print(f"[db_copy] Copying {repo_db} → {ARMOR}/{DB_NAME}")
        print(ssh(c, f"cp {repo_db} {ARMOR}/{DB_NAME} && echo OK || echo FAILED"))
    else:
        print(f"[db] {DB_NAME} not found on i7 — will be created fresh on ArmorATD when engine runs")

# 8. Update repo symlinks on i7 to point at /mnt/cesarops-armor/downloads
print("\n[symlinks] Updating repo download symlinks to point at ArmorATD...")
for lake in ["michigan", "superior", "huron", "erie"]:
    link = f"/home/cesarops/wreckhunter2000-1/downloads/{lake}"
    target = f"{DST_DOWNLOADS}/{lake}"
    print(ssh(c, f"[ -d {target} ] && ln -sfn {target} {link} && echo 'updated {lake}' || echo 'no {lake} dir'"))

c.close()
print("\n=== DONE ===")
print(f"TIFs accessible at: {DST_DOWNLOADS}")
print(f"DB location: {ARMOR}/{DB_NAME}")
print(f"Samba share:  \\\\{I7}\\cesarops-armor  →  Z: on Windows")
