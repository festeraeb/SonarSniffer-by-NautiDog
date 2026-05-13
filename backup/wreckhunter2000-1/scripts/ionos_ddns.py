#!/usr/bin/env python3
"""
IONOS Dynamic DNS updater — runs on the Pi (always on Xfinity home network).

Detects the current public WAN IP and updates the IONOS DNS A record for the
configured hostname (default: home.cesarops.com or @ for the apex).

Usage:
    python ionos_ddns.py                  # check & update if changed
    python ionos_ddns.py --force          # update regardless of change
    python ionos_ddns.py --status         # print current IP + DNS, no writes
    python ionos_ddns.py --install-cron   # add cron entry (run every 5 min)

Cron (add manually if --install-cron is unavailable):
    */5 * * * * /usr/bin/python3 /home/pi/wreckhunter2000-1/scripts/ionos_ddns.py >> /var/log/ionos_ddns.log 2>&1

Environment (.env or shell):
    IONOS_API_KEY     — "{prefix}.{secret}"  (from IONOS Developer portal)
    IONOS_ZONE        — DNS zone name, e.g. "cesarops.com"
    IONOS_RECORD_NAME — record to update, e.g. "home" or "@" (default: "home")
    IONOS_TTL         — TTL in seconds (default: 60)
"""

from __future__ import annotations

import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.error
from pathlib import Path

# ── Load .env ──────────────────────────────────────────────────────────────────
def _load_env(path: Path) -> dict:
    env: dict = {}
    if path.exists():
        for line in path.read_text().splitlines():
            line = line.strip()
            if not line or line.startswith("#") or "=" not in line:
                continue
            k, _, v = line.partition("=")
            env[k.strip()] = v.strip()
    return env

_dotenv = _load_env(Path(__file__).parent.parent / ".env")

def _cfg(key: str, default: str = "") -> str:
    return os.environ.get(key, _dotenv.get(key, default))


# ── Config ─────────────────────────────────────────────────────────────────────
IONOS_API_KEY    = _cfg("IONOS_API_KEY")
IONOS_ZONE       = _cfg("IONOS_ZONE",        "cesarops.com")
IONOS_RECORD     = _cfg("IONOS_RECORD_NAME", "home")
IONOS_TTL        = int(_cfg("IONOS_TTL",     "300"))  # IONOS minimum TTL is 300
IONOS_BASE       = "https://api.hosting.ionos.com/dns/v1"

# Public-IP probe services — IPv4-only endpoints (tried in order)
_IP_PROBES = [
    "https://api4.my-ip.io/ip",       # explicitly v4
    "https://ipv4.icanhazip.com",      # explicitly v4
    "https://ipv4.wtfismyip.com/text", # explicitly v4
    "https://ifconfig.me/ip",          # falls back to v4 if v6 absent
]

import re as _re
_IPV4_RE = _re.compile(r"^\d{1,3}(\.\d{1,3}){3}$")

CACHE_FILE = Path("/tmp/ionos_ddns_last.txt")  # last-known IP cache on Pi


# ── Helpers ────────────────────────────────────────────────────────────────────
def _get(url: str, headers: dict | None = None, timeout: int = 10) -> tuple[int, bytes]:
    req = urllib.request.Request(url, headers=headers or {})
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def _request(method: str, url: str, payload, headers: dict) -> tuple[int, bytes]:
    data = json.dumps(payload).encode()
    headers = {**headers, "Content-Type": "application/json"}
    req = urllib.request.Request(url, data=data, headers=headers, method=method)
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read()


def get_wan_ip() -> str:
    """Fetch current public IPv4 from multiple probe services."""
    for probe in _IP_PROBES:
        try:
            status, body = _get(probe, timeout=8)
            if status == 200:
                ip = body.decode().strip()
                if _IPV4_RE.match(ip):
                    return ip
        except Exception:
            continue
    raise RuntimeError("All WAN IPv4 probes failed — check connectivity")


def _auth_headers() -> dict:
    if not IONOS_API_KEY:
        raise RuntimeError(
            "IONOS_API_KEY not set. Add it to .env as  IONOS_API_KEY={prefix}.{secret}"
        )
    return {"X-API-Key": IONOS_API_KEY, "Accept": "application/json"}


def get_zone_id(zone_name: str) -> str:
    """Return the IONOS zone ID for the given domain name."""
    status, body = _get(f"{IONOS_BASE}/zones", headers=_auth_headers())
    if status != 200:
        raise RuntimeError(f"IONOS zones list failed ({status}): {body.decode()[:300]}")
    zones = json.loads(body)
    for z in zones:
        if z.get("name", "").rstrip(".").lower() == zone_name.lower():
            return z["id"]
    raise RuntimeError(
        f"Zone '{zone_name}' not found in IONOS account. "
        f"Available: {[z.get('name') for z in zones]}"
    )


def get_current_record(zone_id: str, record_name: str) -> tuple[str | None, str | None]:
    """Return (record_id, current_ip) for the A record, or (None, None) if absent."""
    status, body = _get(
        f"{IONOS_BASE}/zones/{zone_id}?recordType=A",
        headers=_auth_headers(),
    )
    if status != 200:
        raise RuntimeError(f"IONOS zone fetch failed ({status}): {body.decode()[:300]}")
    data   = json.loads(body)
    target = record_name.lstrip("@").lower() or ""  # "@" apex → empty string match
    for rec in data.get("records", []):
        if rec.get("type") != "A":
            continue
        rname = rec.get("name", "").lower()
        # apex record stored as zone name or empty
        if target == "" and rname in ("", data.get("name", "").rstrip(".")):
            return rec["id"], rec["content"]
        if rname == target:
            return rec["id"], rec["content"]
    return None, None


def upsert_record(zone_id: str, record_id: str | None, record_name: str, ip: str) -> None:
    """Create (POST) or update (PUT) the A record to point at ip."""
    record_body = {
        "name":     f"{record_name}.{IONOS_ZONE}" if record_name not in ("@", "") else IONOS_ZONE,
        "type":     "A",
        "content":  ip,
        "ttl":      IONOS_TTL,
        "prio":     0,
        "disabled": False,
    }
    if record_id:
        # Update existing record — PUT expects single object
        status, body = _request(
            "PUT",
            f"{IONOS_BASE}/zones/{zone_id}/records/{record_id}",
            record_body,
            headers=_auth_headers(),
        )
    else:
        # Create new record — POST expects array
        status, body = _request(
            "POST",
            f"{IONOS_BASE}/zones/{zone_id}/records",
            [record_body],
            headers=_auth_headers(),
        )
    if status not in (200, 201, 204):
        raise RuntimeError(f"IONOS record update failed ({status}): {body.decode()[:400]}")


def _load_cache() -> str:
    try:
        return CACHE_FILE.read_text().strip()
    except Exception:
        return ""


def _save_cache(ip: str) -> None:
    try:
        CACHE_FILE.write_text(ip)
    except Exception:
        pass


# ── Main ───────────────────────────────────────────────────────────────────────
def main() -> int:
    parser = argparse.ArgumentParser(description="IONOS Dynamic DNS updater")
    parser.add_argument("--force",        action="store_true", help="Update even if IP unchanged")
    parser.add_argument("--status",       action="store_true", help="Print current WAN IP + DNS record, no changes")
    parser.add_argument("--install-cron", action="store_true", help="Add crontab entry (every 5 min)")
    args = parser.parse_args()

    if args.install_cron:
        script = str(Path(__file__).resolve())
        entry  = f"*/5 * * * * /usr/bin/python3 {script} >> /var/log/ionos_ddns.log 2>&1"
        import subprocess
        existing = subprocess.run(["crontab", "-l"], capture_output=True, text=True).stdout
        if script in existing:
            print("Cron entry already installed.")
        else:
            new_cron = existing.rstrip("\n") + "\n" + entry + "\n"
            subprocess.run(["crontab", "-"], input=new_cron, text=True, check=True)
            print(f"Cron installed: {entry}")
        return 0

    # Get WAN IP
    try:
        wan_ip = get_wan_ip()
    except RuntimeError as e:
        print(f"[DDNS] ERROR: {e}", flush=True)
        return 1

    # Status mode: just print, no writes
    if args.status:
        print(f"[DDNS] WAN IP (Xfinity): {wan_ip}")
        try:
            zone_id = get_zone_id(IONOS_ZONE)
            _, dns_ip = get_current_record(zone_id, IONOS_RECORD)
            fqdn = f"{IONOS_RECORD}.{IONOS_ZONE}" if IONOS_RECORD != "@" else IONOS_ZONE
            print(f"[DDNS] DNS   {fqdn} A → {dns_ip or '(not set)'}")
            if wan_ip == dns_ip:
                print("[DDNS] In sync ✓")
            else:
                print("[DDNS] OUT OF SYNC — run without --status to fix")
        except RuntimeError as e:
            print(f"[DDNS] DNS lookup error: {e}")
        return 0

    # Check cache to skip unnecessary API calls
    cached_ip = _load_cache()
    if cached_ip == wan_ip and not args.force:
        print(f"[DDNS] IP unchanged ({wan_ip}) — skipping update", flush=True)
        return 0

    # Resolve zone and current record
    try:
        zone_id             = get_zone_id(IONOS_ZONE)
        record_id, dns_ip   = get_current_record(zone_id, IONOS_RECORD)
    except RuntimeError as e:
        print(f"[DDNS] ERROR: {e}", flush=True)
        return 1

    if dns_ip == wan_ip and not args.force:
        _save_cache(wan_ip)
        fqdn = f"{IONOS_RECORD}.{IONOS_ZONE}" if IONOS_RECORD != "@" else IONOS_ZONE
        print(f"[DDNS] {fqdn} already → {wan_ip} — no update needed", flush=True)
        return 0

    # Push update
    try:
        upsert_record(zone_id, record_id, IONOS_RECORD, wan_ip)
        _save_cache(wan_ip)
        fqdn = f"{IONOS_RECORD}.{IONOS_ZONE}" if IONOS_RECORD != "@" else IONOS_ZONE
        ts   = time.strftime("%Y-%m-%d %H:%M:%S UTC", time.gmtime())
        print(f"[DDNS] {ts}  Updated {fqdn} → {wan_ip}  (was: {dns_ip or 'none'})", flush=True)
    except RuntimeError as e:
        print(f"[DDNS] ERROR updating record: {e}", flush=True)
        return 1

    return 0


if __name__ == "__main__":
    sys.exit(main())
