#!/usr/bin/env python3
"""
Deploy tauri/dist-web/ static site (index, tools.html, mission-control).

Targets:
    (default) T440 /var/www/cesarops  -> https://app.cesarops.org
    --ionos   IONOS SFTP /wh2000/     -> https://cesarops.com/wh2000/
    --backup  cesarops3 (retired backup host)

Usage:
    python scripts/deploy_web.py                 # T440 (primary)
    python scripts/deploy_web.py --ionos         # IONOS public site
    python scripts/deploy_web.py --dry-run       # list files only
    python scripts/deploy_web.py --ionos --dry-run
    python scripts/deploy_web.py --build         # npm run build:web first (if tauri web app exists)

Reads from repo .env (symlink on NFS: /data/cesarops/repo/.env on T440):
    T440_TAILSCALE, T440_USER, T440_PASS
    IONOS_SFTP_HOST, IONOS_SFTP_USER, IONOS_SFTP_PASS, IONOS_SFTP_PORT
    P1000_TAILSCALE, P1000_PASS  (--backup only)
"""

import argparse
import os
import subprocess
import sys
from pathlib import Path

import paramiko
from dotenv import load_dotenv

REPO = Path(__file__).resolve().parents[1]
DIST_DIR = REPO / "tauri" / "dist-web"
REMOTE_DIR = "/wh2000"   # path inside public_html on IONOS

load_dotenv(REPO / ".env")

SFTP_HOST = os.environ.get("IONOS_SFTP_HOST", "access-5019147877.webspace-host.com")
SFTP_USER = os.environ.get("IONOS_SFTP_USER", "a1268970")
SFTP_PASS = os.environ.get("IONOS_SFTP_PASS", "")
SFTP_PORT = int(os.environ.get("IONOS_SFTP_PORT", "22"))


def build_web():
    print("[BUILD] Running npm run build:web …")
    result = subprocess.run(
        ["npm", "run", "build:web"],
        cwd=str(REPO / "tauri"),
        shell=True,
    )
    if result.returncode != 0:
        sys.exit(f"[BUILD] Failed with exit code {result.returncode}")
    print("[BUILD] Done.")


def sftp_mkdir_p(sftp, remote_path: str):
    """Ensure remote directory exists, creating parent dirs as needed."""
    parts = [p for p in remote_path.split("/") if p]
    current = "/"
    for part in parts:
        current = current.rstrip("/") + "/" + part
        try:
            sftp.stat(current)
        except FileNotFoundError:
            sftp.mkdir(current)


def upload(dry_run: bool = False):
    if not SFTP_PASS:
        sys.exit(
            "[ERROR] IONOS_SFTP_PASS is empty.\n"
            "        Set it in .env:  IONOS_SFTP_PASS=yourpassword\n"
            "        Or reset via IONOS control panel and update .env."
        )

    if not DIST_DIR.exists():
        sys.exit(
            f"[ERROR] dist-web/ not found at {DIST_DIR}\n"
            "        Run:  python scripts/deploy_web.py --build"
        )

    # Collect all files to upload
    files = sorted(DIST_DIR.rglob("*"))
    files = [f for f in files if f.is_file()]
    print(f"[UPLOAD] {len(files)} files -> {SFTP_HOST}:{REMOTE_DIR}/")

    if dry_run:
        for f in files:
            rel = f.relative_to(DIST_DIR)
            print(f"  [DRY] {rel}")
        return

    transport = paramiko.Transport((SFTP_HOST, SFTP_PORT))
    transport.connect(username=SFTP_USER, password=SFTP_PASS)
    sftp = paramiko.SFTPClient.from_transport(transport)

    try:
        sftp_mkdir_p(sftp, REMOTE_DIR)

        for local_file in files:
            rel = local_file.relative_to(DIST_DIR)
            rel_str = str(rel).replace("\\", "/")
            # vite-plugin-cesium mirrors the base path (/wh2000/) into dist-web/wh2000/
            # Strip that leading prefix so cesium lands at /wh2000/cesium/ not /wh2000/wh2000/cesium/
            BASE_PREFIX = REMOTE_DIR.lstrip("/") + "/"
            if rel_str.startswith(BASE_PREFIX):
                rel_str = rel_str[len(BASE_PREFIX):]
            remote_file = (REMOTE_DIR + "/" + rel_str)

            # Ensure parent directory exists
            parent = remote_file.rsplit("/", 1)[0]
            sftp_mkdir_p(sftp, parent)

            print(f"  -> {remote_file}")
            sftp.put(str(local_file), remote_file)

        print(f"\n[DONE] Deployed to https://cesarops.com/wh2000/")
        print("       https://cesarops.com/wh2000/tools.html")
        print("       https://cesarops.com/wh2000/mission-control/")
    finally:
        sftp.close()
        transport.close()


def deploy_t440(dry_run: bool = False):
    """Deploy to T440 via SCP (PRIMARY target for app.cesarops.org)."""
    if not DIST_DIR.exists():
        sys.exit(
            f"[ERROR] dist-web/ not found at {DIST_DIR}\n"
            "        Run:  python scripts/deploy_web.py --build"
        )

    files = sorted(DIST_DIR.rglob("*"))
    files = [f for f in files if f.is_file()]
    print(f"[UPLOAD] {len(files)} files -> T440:/var/www/cesarops/")

    if dry_run:
        for f in files:
            rel = f.relative_to(DIST_DIR)
            print(f"  [DRY] {rel}")
        return

    T440_HOST = os.environ.get("T440_TAILSCALE", "100.72.182.77")
    T440_USER = os.environ.get("T440_USER", "cesarops")
    T440_PASS = os.environ.get("T440_PASS", "cesarops")
    REMOTE_WEB_DIR = "/var/www/cesarops"

    transport = paramiko.Transport((T440_HOST, 22))
    transport.connect(username=T440_USER, password=T440_PASS)
    sftp = paramiko.SFTPClient.from_transport(transport)

    try:
        for local_file in files:
            rel = local_file.relative_to(DIST_DIR)
            rel_str = str(rel).replace("\\", "/")
            remote_file = f"{REMOTE_WEB_DIR}/{rel_str}"

            parent = remote_file.rsplit("/", 1)[0]
            sftp_mkdir_p(sftp, parent)

            print(f"  -> {remote_file}")
            sftp.put(str(local_file), remote_file)

        print(f"\n[DONE] Deployed to https://app.cesarops.org (T440)")
        print("       /tools.html  /mission-control/  (under site root)")
    finally:
        sftp.close()
        transport.close()


def deploy_cesarops3(dry_run: bool = False):
    """Deploy to cesarops3 via SCP (BACKUP for app.cesarops.org)."""
    if not DIST_DIR.exists():
        sys.exit(
            f"[ERROR] dist-web/ not found at {DIST_DIR}\n"
            "        Run:  python scripts/deploy_web.py --build"
        )

    files = sorted(DIST_DIR.rglob("*"))
    files = [f for f in files if f.is_file()]
    print(f"[UPLOAD] {len(files)} files -> cesarops3:/var/www/cesarops/ (backup)")

    if dry_run:
        for f in files:
            rel = f.relative_to(DIST_DIR)
            print(f"  [DRY] {rel}")
        return

    CESAROPS3_HOST = os.environ.get("P1000_TAILSCALE", "100.105.77.74")
    CESAROPS3_USER = "cesarops"
    CESAROPS3_PASS = os.environ.get("P1000_PASS", "cesarops")
    REMOTE_WEB_DIR = "/var/www/cesarops"

    transport = paramiko.Transport((CESAROPS3_HOST, 22))
    transport.connect(username=CESAROPS3_USER, password=CESAROPS3_PASS)
    sftp = paramiko.SFTPClient.from_transport(transport)

    try:
        for local_file in files:
            rel = local_file.relative_to(DIST_DIR)
            rel_str = str(rel).replace("\\", "/")
            remote_file = f"{REMOTE_WEB_DIR}/{rel_str}"

            parent = remote_file.rsplit("/", 1)[0]
            sftp_mkdir_p(sftp, parent)

            print(f"  -> {remote_file}")
            sftp.put(str(local_file), remote_file)

        print(f"\n[DONE] Deployed to cesarops3 (backup)")
    finally:
        sftp.close()
        transport.close()


def main():
    parser = argparse.ArgumentParser(description="Deploy WH2000 web build")
    parser.add_argument("--build",   action="store_true", help="Run npm run build:web before upload")
    parser.add_argument("--dry-run", action="store_true", help="List files without uploading")
    parser.add_argument("--ionos",   action="store_true", help="Deploy to IONOS SFTP (legacy)")
    parser.add_argument("--backup",  action="store_true", help="Deploy to cesarops3 (backup)")
    args = parser.parse_args()

    if args.build:
        build_web()

    if args.ionos:
        upload(dry_run=args.dry_run)
    elif args.backup:
        deploy_cesarops3(dry_run=args.dry_run)
    else:
        deploy_t440(dry_run=args.dry_run)


if __name__ == "__main__":
    main()
