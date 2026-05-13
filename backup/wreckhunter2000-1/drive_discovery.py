"""
CESAROPS Drive Discovery
------------------------
Finds the WreckHunter ArmorATD drive regardless of which machine it's plugged
into, using a two-stage strategy:

  Stage 1 – LOCAL:  Check if the partition UUID is mounted on this machine.
  Stage 2 – NETWORK: ARP-scan for known host MAC addresses; for each found
                     host, try the Samba share //host/cesarops-armor.
                     Falls back to SSHFS if SMB isn't available.

Config is in drive_config.json at the repo root.  All values can be
overridden via environment variables (see ENVIRON section below).

Returns a Path object to the drive root, or None if not found.
"""

import json
import os
import platform
import socket
import subprocess
import sys
import time
from pathlib import Path

# ── Config ────────────────────────────────────────────────────────────────
_HERE = Path(__file__).parent

# drive_config.json lives at repo root
_CONFIG_FILE = _HERE / "drive_config.json"

_DEFAULT_CONFIG = {
    # Partition UUID of the ArmorATD data partition (ext4, label wreckhunter-data)
    "armor_uuid": "dec00b8b-a95a-4c02-ae40-f7e6ab1b21e9",

    # Samba share name advertised by the host
    "smb_share": "cesarops-armor",

    # Known hosts that might hold the drive (MAC → metadata)
    "known_hosts": {
        "b8:ca:3a:9e:b5:ec": {"name": "i7",   "user": "cesarops",  "pw": "cesarops",  "mount": "/mnt/cesarops-armor"},
        "d0:50:99:59:2a:f1": {"name": "xeon",  "user": "cesarops1", "pw": "cesarops1", "mount": "/mnt/cesarops-armor"},
    },

    # Where to mount on THIS machine when accessed over network
    "local_smb_mount":   "/mnt/cesarops-armor",      # Linux
    "local_sshfs_mount": "/mnt/cesarops-armor",      # Linux fallback
    "windows_smb_drive": "Z:",                        # Windows: map as Z:
}

# ── Environment overrides ─────────────────────────────────────────────────
# CESAROPS_DRIVE  — explicit path, skips all discovery
# CESAROPS_ARMOR_HOST — explicit IP/hostname of the drive host


def _load_config() -> dict:
    cfg = dict(_DEFAULT_CONFIG)
    if _CONFIG_FILE.exists():
        try:
            with open(_CONFIG_FILE) as f:
                cfg.update(json.load(f))
        except Exception:
            pass
    return cfg


def _is_windows() -> bool:
    return platform.system() == "Windows"


# ── Stage 1: local UUID check ─────────────────────────────────────────────

def _find_local_uuid_mount(uuid: str) -> Path | None:
    """Return mount point if the partition UUID is already mounted locally."""
    if _is_windows():
        # On Windows check all drive letters for the DB sentinel file
        # (Windows mounts by drive letter, not UUID natively)
        return None
    try:
        out = subprocess.check_output(
            ["findmnt", "-n", "-o", "TARGET", f"UUID={uuid}"],
            stderr=subprocess.DEVNULL,
            text=True,
        ).strip()
        if out:
            return Path(out)
    except (FileNotFoundError, subprocess.CalledProcessError):
        pass
    # fallback: scan /proc/mounts
    try:
        dev = subprocess.check_output(
            ["blkid", "-U", uuid], stderr=subprocess.DEVNULL, text=True
        ).strip()
        if dev:
            with open("/proc/mounts") as f:
                for line in f:
                    parts = line.split()
                    if parts and parts[0] == dev:
                        return Path(parts[1])
    except Exception:
        pass
    return None


# ── Stage 2: ARP-based network discovery ─────────────────────────────────

def _arp_table() -> dict[str, str]:
    """Return {mac: ip} from the local ARP cache."""
    mac_to_ip: dict[str, str] = {}
    try:
        if _is_windows():
            out = subprocess.check_output(["arp", "-a"], text=True, stderr=subprocess.DEVNULL)
            for line in out.splitlines():
                parts = line.split()
                # Windows: "  10.0.0.56    b8-ca-3a-9e-b5-ec  dynamic"
                if len(parts) >= 2:
                    ip  = parts[0].strip()
                    mac = parts[1].strip().replace("-", ":").lower()
                    if len(mac) == 17:
                        mac_to_ip[mac] = ip
        else:
            out = subprocess.check_output(["arp", "-n"], text=True, stderr=subprocess.DEVNULL)
            for line in out.splitlines():
                parts = line.split()
                # Linux: "10.0.0.56  ether  b8:ca:3a:9e:b5:ec  C  eth0"
                if len(parts) >= 3 and len(parts[2]) == 17:
                    mac_to_ip[parts[2].lower()] = parts[0]
    except Exception:
        pass
    return mac_to_ip


def _ping_subnet(subnet: str = "10.0.0") -> None:
    """Quickly ping the subnet to populate the ARP cache."""
    try:
        if _is_windows():
            for i in range(1, 255):
                subprocess.Popen(
                    ["ping", "-n", "1", "-w", "150", f"{subnet}.{i}"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                )
        else:
            for i in range(1, 255):
                subprocess.Popen(
                    ["ping", "-c", "1", "-W", "1", f"{subnet}.{i}"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
                )
        time.sleep(1.5)  # let pings return
    except Exception:
        pass


def _host_reachable(ip: str, port: int = 22, timeout: float = 1.5) -> bool:
    try:
        s = socket.create_connection((ip, port), timeout=timeout)
        s.close()
        return True
    except OSError:
        return False


# ── Stage 2a: Windows SMB mount ──────────────────────────────────────────

def _try_smb_windows(ip: str, share: str, drive_letter: str, user: str, pw: str) -> Path | None:
    unc = f"\\\\{ip}\\{share}"
    # Disconnect first in case stale
    subprocess.call(["net", "use", drive_letter, "/delete", "/y"],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ret = subprocess.call(
        ["net", "use", drive_letter, unc, f"/user:{user}", pw, "/persistent:yes"],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    if ret == 0:
        p = Path(f"{drive_letter}\\")
        if p.exists():
            return p
    return None


# ── Stage 2b: Linux SSHFS mount ──────────────────────────────────────────

def _try_sshfs_linux(ip: str, remote_path: str, local_mount: str, user: str) -> Path | None:
    os.makedirs(local_mount, exist_ok=True)
    # Unmount if stale
    subprocess.call(["fusermount", "-u", local_mount],
                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    ret = subprocess.call([
        "sshfs",
        "-o", "StrictHostKeyChecking=no,reconnect,ServerAliveInterval=15",
        f"{user}@{ip}:{remote_path}",
        local_mount,
    ], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if ret == 0:
        return Path(local_mount)
    return None


# ── Main discovery entry point ────────────────────────────────────────────

def find_armor_drive(quiet: bool = False) -> Path | None:
    """
    Return a Path to the ArmorATD drive root, or None.
    Sets os.environ['CESAROPS_DRIVE'] on success so subsequent imports
    in the same process can find it without re-scanning.
    """

    def log(msg: str) -> None:
        if not quiet:
            print(f"[armor] {msg}")

    # 0. Explicit env override
    if explicit := os.environ.get("CESAROPS_DRIVE"):
        p = Path(explicit)
        if p.exists():
            log(f"Using CESAROPS_DRIVE override: {p}")
            return p

    cfg = _load_config()
    uuid   = cfg["armor_uuid"]
    share  = cfg["smb_share"]
    hosts  = cfg["known_hosts"]

    # 1. Already mounted locally?
    local = _find_local_uuid_mount(uuid)
    if local:
        log(f"Found locally mounted at {local}")
        os.environ["CESAROPS_DRIVE"] = str(local)
        return local

    # 2. Windows drive letter scan (portable plug-in scenario)
    if _is_windows():
        db_name = "LAKE_MICHIGAN_CENSUS_2026.db"
        for letter in "DEFGHIJKLMNOPQRSTUVWXYZ":
            p = Path(f"{letter}:\\")
            if (p / db_name).exists():
                log(f"Found on Windows drive {letter}:")
                os.environ["CESAROPS_DRIVE"] = str(p)
                return p

    # 3. Network: find host by MAC
    explicit_host = os.environ.get("CESAROPS_ARMOR_HOST")
    target_ip = None
    target_meta = None

    if explicit_host:
        target_ip   = explicit_host
        target_meta = next(iter(hosts.values()))  # use first host's creds as default
        log(f"Using explicit CESAROPS_ARMOR_HOST={explicit_host}")
    else:
        log("Scanning ARP for known hosts...")
        arp = _arp_table()
        # Try to find known MACs without a full ping-sweep first
        for mac, meta in hosts.items():
            if mac in arp:
                target_ip   = arp[mac]
                target_meta = meta
                log(f"Found {meta['name']} ({mac}) at {target_ip}")
                break

        if not target_ip:
            # ARP cache miss — do a fast ping sweep to populate it
            log("ARP cache miss — pinging subnet to populate...")
            _ping_subnet()
            arp = _arp_table()
            for mac, meta in hosts.items():
                if mac in arp:
                    target_ip   = arp[mac]
                    target_meta = meta
                    log(f"Found {meta['name']} ({mac}) at {target_ip} after sweep")
                    break

    if not target_ip:
        log("No known host found on network — drive not available")
        return None

    if not _host_reachable(target_ip):
        log(f"{target_ip} is not reachable on port 22")
        return None

    # 4. Mount it
    if _is_windows():
        drive_letter = cfg.get("windows_smb_drive", "Z:")
        log(f"Mounting \\\\{target_ip}\\{share} as {drive_letter} ...")
        p = _try_smb_windows(target_ip, share,
                             drive_letter,
                             target_meta["user"],
                             target_meta["pw"])
        if p:
            log(f"Mounted at {p}")
            os.environ["CESAROPS_DRIVE"] = str(p)
            return p
        log("SMB mount failed")
    else:
        local_mount = cfg.get("local_sshfs_mount", "/mnt/cesarops-armor")
        remote_path = target_meta.get("mount", "/mnt/cesarops-armor")
        log(f"Mounting {target_meta['user']}@{target_ip}:{remote_path} → {local_mount} ...")
        p = _try_sshfs_linux(target_ip, remote_path, local_mount, target_meta["user"])
        if p:
            log(f"Mounted at {p}")
            os.environ["CESAROPS_DRIVE"] = str(p)
            return p
        log("SSHFS mount failed")

    return None


if __name__ == "__main__":
    result = find_armor_drive(quiet=False)
    if result:
        print(f"\nDrive available at: {result}")
        print(f"DB: {result / 'LAKE_MICHIGAN_CENSUS_2026.db'} exists={( result / 'LAKE_MICHIGAN_CENSUS_2026.db').exists()}")
    else:
        print("\nDrive NOT found.")
        sys.exit(1)
