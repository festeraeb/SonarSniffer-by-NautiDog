#!/usr/bin/env python3
"""
Patch failed/queued jobs that have empty bbox/params.

These jobs were originally submitted without dates or coordinates.
The correct data is sourced from the known target areas in the agent context.

Run: python scripts/patch_failed_jobs.py [--dry-run] [--pi http://...]
"""
import argparse
import json
import sys
import urllib.request
from pathlib import Path

# Bboxes: [lat_min, lon_min, lat_max, lon_max]
# date_start / date_end: best clear-water summer window for each area
PATCHES = {
    "hormuz_full_sar":    dict(bbox=[25.0, 54.0, 27.5, 58.5], date_start="2025-03-01", date_end="2025-03-31", job_type="gpu"),
    "hormuz_bottleneck":  dict(bbox=[26.1, 56.3, 26.8, 57.0], date_start="2025-03-01", date_end="2025-03-31", job_type="gpu"),
    "hormuz_abu_musa":    dict(bbox=[25.7, 55.0, 25.95, 55.2], date_start="2025-03-01", date_end="2025-03-31", job_type="gpu"),
    "nome_cessna_primary": dict(bbox=[64.3,-165.8, 64.9,-164.8], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "nome_cessna_shore":   dict(bbox=[64.4,-165.5, 64.75,-164.9], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "nome_cessna_norton":  dict(bbox=[64.2,-164.5, 64.7,-163.8], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "alaska_canopy_areaA": dict(bbox=[64.0,-153.0, 65.0,-151.5], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "alaska_canopy_areaB": dict(bbox=[63.5,-153.5, 64.5,-152.0], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "alaska_canopy_areaC": dict(bbox=[63.0,-154.0, 64.0,-152.5], date_start="2025-07-01", date_end="2025-09-15", job_type="gpu"),
    "NWA_2501_primary":    dict(bbox=[41.0,-68.5, 42.5,-66.5], date_start="2025-06-01", date_end="2025-09-30", job_type="gpu"),
    "NWA_2501_extended":   dict(bbox=[40.5,-69.5, 43.0,-65.5], date_start="2025-06-01", date_end="2025-09-30", job_type="gpu"),
}


def _api(base: str, method: str, path: str, body: dict | None = None, timeout: int = 10) -> dict:
    url = f"{base}{path}"
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"} if data else {},
        method=method,
    )
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return json.loads(r.read())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--pi", default="http://100.127.66.32:8099")
    ap.add_argument("--status", default="FAILED", help="Patch jobs with this status (default FAILED). Use ALL for any.")
    args = ap.parse_args()

    data = _api(args.pi, "GET", "/jobs")
    all_jobs = data.get("jobs", data) if isinstance(data, dict) else data

    for job in all_jobs:
        label = job.get("label", "")
        status = job.get("status", "")
        bbox = job.get("bbox") or []
        params = job.get("params") or {}

        patch = PATCHES.get(label)
        if not patch:
            continue  # not in our known table

        target_status = args.status.upper()
        if target_status != "ALL" and status != target_status:
            continue

        # Determine what needs fixing
        needs_bbox = not bbox or len(bbox) != 4
        needs_dates = not params.get("date_start") or not params.get("date_end")

        if not needs_bbox and not needs_dates:
            print(f"OK     [{status}] {label} — bbox and dates already set")
            continue

        print(f"PATCH  [{status}] {label} (id={job['id'][:8]})")
        if needs_bbox:
            print(f"         bbox: [] -> {patch['bbox']}")
        if needs_dates:
            print(f"         date: missing -> {patch['date_start']} .. {patch['date_end']}")

        if args.dry_run:
            continue

        # Re-submit as a fresh QUEUED job with correct data.
        # First cancel the broken one.
        try:
            _api(args.pi, "POST", f"/jobs/{job['id']}/finish", {"success": False, "error_msg": "Re-queued with corrected bbox/params"})
        except Exception as e:
            print(f"         warn: could not mark old job failed: {e}")

        new_params = dict(params)
        new_params.setdefault("date_start", patch["date_start"])
        new_params.setdefault("date_end", patch["date_end"])

        resp = _api(args.pi, "POST", "/jobs", {
            "label":    label,
            "bbox":     patch["bbox"],
            "sensors":  job.get("sensors") or ["sar"],
            "params":   new_params,
            "job_type": patch["job_type"],
            "priority": job.get("priority") or 1,
        })
        print(f"         -> new id={resp.get('id','?')}")

    print("\nDone.")


if __name__ == "__main__":
    main()
