#!/usr/bin/env python3
"""
CESAROPS Remote Job Runner — executes a single scan_job end-to-end on i7.

Called by queue_worker.py via SSH:
    python run_job_remote.py --job-json '{"id":"...","label":"...","bbox":[...],"sensors":[...],"params":{...}}'

Steps:
    1. Parse job JSON
    2. Download required satellite data via universal_downloader.py
    3. Run mission analysis via cesarops_mission.run_mission()
    4. Print final result as JSON on its own line
    5. Exit 0 on success, 1 on failure
"""

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent


# ── Pass configuration ────────────────────────────────────────────────────────

def build_mission_passes(sensors: list, target_type: str = "") -> dict:
    """Map sensor list + target_type to mission analysis pass config."""
    passes = {
        "standard":        {"enabled": True,  "threshold": 1.5},
        "nauticuvs":       {"enabled": True,  "energy_threshold": 3.5, "top_n": 50},
        "hydrocarbon":     {"enabled": False, "swir_thresh": -1.8, "red_thresh": 1.5},
        "thermal":         {"enabled": False, "threshold": 2.0},
        "stumpf":          {"enabled": False, "threshold": 2.0},
        "swir_silt_erasure": {"enabled": False, "threshold": 2.5, "top_n": 30},
        "mussel_clearspot":  {"enabled": False, "threshold": 2.0, "top_n": 30},
    }

    if "optical" in sensors:
        passes["hydrocarbon"]["enabled"] = True
        passes["thermal"]["enabled"] = True

    if "nir_swir" in sensors:
        passes["hydrocarbon"]["enabled"] = True
        passes["swir_silt_erasure"]["enabled"] = True

    if target_type in ("bathymetric", "wreck"):
        passes["stumpf"]["enabled"] = True

    return passes


# ── Download step ─────────────────────────────────────────────────────────────

def download_data(label: str, bbox: list, sensors: list, date_start: str, date_end: str) -> Path:
    """Run universal_downloader.py for this job. Returns output directory."""
    safe_label = re.sub(r"[^\w\-]", "_", label)
    out_dir = REPO / "downloads" / "jobs" / safe_label
    out_dir.mkdir(parents=True, exist_ok=True)

    bbox_str    = f"{bbox[0]},{bbox[1]},{bbox[2]},{bbox[3]}"
    sensors_str = ",".join(sensors)

    cmd = [
        sys.executable,
        str(REPO / "universal_downloader.py"),
        "--bbox",        bbox_str,
        "--dates",       date_start, date_end,
        "--sensors",     sensors_str,
        "--output",      str(out_dir),
        "--max-results", "30",
    ]

    print(
        f"[DOWNLOAD] {label}: sensors={sensors_str} | "
        f"{date_start} → {date_end} | bbox={bbox_str}",
        flush=True,
    )

    result = subprocess.run(cmd, text=True)
    if result.returncode != 0:
        print(
            f"[DOWNLOAD] {label}: downloader exited {result.returncode} (no data or partial) — continuing",
            flush=True,
        )

    tif_count = sum(1 for _ in out_dir.rglob("*.tif"))
    zip_count = sum(1 for _ in out_dir.rglob("*.zip"))
    total = tif_count + zip_count
    if total == 0:
        print(
            f"[DOWNLOAD] {label}: 0 files processed, no data for assigned dates ({date_start} \u2192 {date_end})",
            flush=True,
        )
    else:
        print(f"[DOWNLOAD] {label}: {tif_count} TIF, {zip_count} ZIP in {out_dir}", flush=True)

    return out_dir


# ── Mission runner ────────────────────────────────────────────────────────────

def run_mission_for_job(job: dict) -> dict:
    """Build mission dict from job row and run the analysis pipeline."""
    from cesarops_mission import run_mission  # noqa: local import — avoids engine load at startup

    label       = job["label"]
    bbox        = job["bbox"]          # list [lat_min, lon_min, lat_max, lon_max]
    sensors     = job["sensors"]       # list e.g. ["sar", "optical"]
    params      = job.get("params", {})

    date_start   = params.get("date_start", "")
    date_end     = params.get("date_end", "")
    target_type  = params.get("target_type", "")
    mission_name = params.get("mission_name", label)

    if not date_start or not date_end:
        raise ValueError(f"Job '{label}' missing date_start/date_end in params: {params}")

    # 1. Download
    out_dir = download_data(label, bbox, sensors, date_start, date_end)

    # Short-circuit: no files downloaded — report clean zero-result
    total_files = sum(1 for _ in out_dir.rglob("*.tif")) + sum(1 for _ in out_dir.rglob("*.zip"))
    if total_files == 0:
        return {
            "detections": 0,
            "result_path": str(REPO / "outputs" / re.sub(r"[^\w\-]", "_", label)),
            "job_id": job.get("id", ""),
            "label": label,
            "note": f"no data downloaded for {date_start} \u2192 {date_end}",
        }

    # 2. Build mission config
    safe_label = re.sub(r"[^\w\-]", "_", label)
    mission = {
        "name":       f"{mission_name} — {label}",
        "bbox":       bbox,
        "output_tag": safe_label,
        "data_dirs":  [str(out_dir)],
        "passes":     build_mission_passes(sensors, target_type),
        "sub_zones":  params.get("sub_zones", []),
    }

    # 3. Run analysis
    result = run_mission(mission)

    result_path = str(REPO / "outputs" / safe_label)
    result["result_path"] = result_path
    result["job_id"]      = job.get("id", "")
    result["label"]       = label

    return result


# ── CLI entry point ───────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="CESAROPS Remote Job Runner")
    parser.add_argument("--job-json", required=True, help="Job dict as JSON string")
    args = parser.parse_args()

    try:
        job = json.loads(args.job_json)
    except json.JSONDecodeError as exc:
        print(json.dumps({"status": "error", "error": f"Invalid JSON: {exc}"}), flush=True)
        return 1

    label  = job.get("label", "unknown")
    job_id = job.get("id", "?")
    print(f"[JOB] Started: {label} (id={job_id[:8]})", flush=True)

    try:
        result = run_mission_for_job(job)
        # Emit the result JSON as the last parseable line so queue_worker can extract it
        print(json.dumps(result), flush=True)
        print(
            f"[JOB] Complete: {label} | detections={result.get('detections', 0)} "
            f"| output={result.get('result_path', '')}",
            flush=True,
        )
        return 0

    except Exception as exc:
        import traceback
        trace = traceback.format_exc()
        print(f"[JOB] FAILED: {label}: {exc}", flush=True)
        # Emit error JSON so queue_worker captures a clean error message
        print(
            json.dumps({"status": "error", "label": label, "error": str(exc), "traceback": trace}),
            flush=True,
        )
        return 1


if __name__ == "__main__":
    sys.exit(main())
