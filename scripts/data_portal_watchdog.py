#!/usr/bin/env python3
"""
Data Portal Watchdog
Periodically checks the health of critical data portal dependencies (NRCan, USGS, NOAA).
Maintains state in var/fleet-health/portal_uptime.json and reports to field ops if down.
"""

import json
import os
import time
from pathlib import Path
import urllib.request
import urllib.error

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
OUT_DIR = REPO / "var/fleet-health"
STATE_FILE = OUT_DIR / "portal_uptime.json"

CRITICAL_SITES = {
    "nrcan_geophysics_portal": "https://geophysical-data.canada.ca/",
    "nrcan_geoscan_database": "https://geoscan.nrcan.gc.ca/",
    "nrcan_ftp_archive": "https://ftp.maps.canada.ca/",
    "usgs_mrdata": "https://mrdata.usgs.gov/",
    "usgs_sciencebase": "https://www.sciencebase.gov/",
    "ontario_data": "https://data.ontario.ca/",
    "noaa_ncei": "https://www.ngdc.noaa.gov/",
    "noaa_oer": "https://archive.oceanexplorer.noaa.gov/"
}

# The endpoint where we send downtime telemetry for "field ops"
TELEMETRY_ENDPOINT = "https://cesarops.com/api/telemetry/downtime"

def log(msg: str):
    print(f"[portal-watchdog] {msg}", flush=True)

def load_state() -> dict:
    if STATE_FILE.is_file():
        try:
            return json.loads(STATE_FILE.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            pass
    return {"sites": {}}

def save_state(state: dict):
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps(state, indent=2), encoding="utf-8")

def check_site(url: str, timeout: float = 10.0) -> dict:
    try:
        # Use a user agent to prevent 403s on some portals
        req = urllib.request.Request(url, headers={'User-Agent': 'Mozilla/5.0 (Cesarops Watchdog)'})
        with urllib.request.urlopen(req, timeout=timeout) as response:
            return {"up": True, "status": response.getcode(), "error": None}
    except urllib.error.HTTPError as e:
        return {"up": False, "status": e.code, "error": str(e)}
    except urllib.error.URLError as e:
        return {"up": False, "status": None, "error": str(e.reason)}
    except Exception as e:
        return {"up": False, "status": None, "error": str(e)}

def report_downtime(site_name: str, url: str, status_info: dict):
    payload = json.dumps({
        "service": "portal_watchdog",
        "site": site_name,
        "url": url,
        "status_code": status_info.get("status"),
        "error": status_info.get("error"),
        "timestamp": int(time.time())
    }).encode("utf-8")

    req = urllib.request.Request(
        TELEMETRY_ENDPOINT,
        data=payload,
        headers={"Content-Type": "application/json"}
    )
    try:
        urllib.request.urlopen(req, timeout=5)
        log(f"Successfully reported downtime for {site_name} to field ops telemetry.")
    except Exception as e:
        log(f"Warning: Failed to report downtime to telemetry endpoint: {e}")

def main():
    state = load_state()
    current_time = int(time.time())
    
    for name, url in CRITICAL_SITES.items():
        log(f"Checking {name}...")
        status_info = check_site(url)
        
        site_state = state["sites"].setdefault(name, {
            "url": url,
            "total_checks": 0,
            "successful_checks": 0,
            "last_down_time": None,
            "currently_up": True
        })
        
        site_state["total_checks"] += 1
        site_state["last_check_time"] = current_time
        site_state["currently_up"] = status_info["up"]
        
        if status_info["up"]:
            site_state["successful_checks"] += 1
            log(f"  OK (Status {status_info['status']})")
        else:
            site_state["last_down_time"] = current_time
            site_state["last_error"] = status_info["error"]
            log(f"  DOWN: {status_info['error']}")
            
            # If it's down, report it
            report_downtime(name, url, status_info)

    save_state(state)
    
    # Summary
    down_count = sum(1 for s in state["sites"].values() if not s["currently_up"])
    total_count = len(CRITICAL_SITES)
    log(f"Check complete. {down_count}/{total_count} sites are currently down.")

if __name__ == "__main__":
    main()
