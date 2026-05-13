#!/usr/bin/env python3
"""
Tag existing QUEUED/RUNNING jobs with the right job_type.

Rules:
  gpu_tpu  — if sensor list includes 'sar' AND params has tpu hinting, or label has 'glint'
  gpu      — sensors include 'sar' or 'optical', or label has 'satellite'/'scan'
  cpu      — mag, bag, postprocess, everything else

Run once after migrate_add_job_type.py.
"""
import sqlite3, json, os, sys
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "db" / "scan_queue.db"
if not DB.exists():
    sys.exit(f"ERROR: {DB} not found")

conn = sqlite3.connect(str(DB))
conn.row_factory = sqlite3.Row
cur = conn.cursor()

rows = cur.execute(
    "SELECT id, label, sensors, params, job_type FROM scan_jobs "
    "WHERE status IN ('QUEUED','RUNNING')"
).fetchall()

updates = []
for r in rows:
    label = (r["label"] or "").lower()
    try:
        sensors = json.loads(r["sensors"] or "[]")
    except Exception:
        sensors = []
    try:
        params = json.loads(r["params"] or "{}")
    except Exception:
        params = {}

    sensors_lower = [s.lower() for s in sensors]
    current = r["job_type"] or "cpu"

    # Only retag if still at default
    if current != "cpu":
        print(f"  skip {r['id'][:8]} ({r['label'][:40]}) — already {current}")
        continue

    if params.get("requires_tpu") or "tpu" in label:
        new_type = "gpu_tpu"
    elif (
        "sar" in sensors_lower
        or "optical" in sensors_lower
        or "satellite" in label
        or "hls" in sensors_lower
        or params.get("requires_gpu")
    ):
        new_type = "gpu"
    elif (
        "mag" in sensors_lower
        or "bag" in sensors_lower
        or "magnetometer" in sensors_lower
        or "mag" in label
        or "bag" in label
    ):
        new_type = "cpu"
    else:
        new_type = "gpu"   # default unknown scan jobs to GPU (safe assumption)

    updates.append((new_type, r["id"]))
    print(f"  {r['id'][:8]} ({r['label'][:40]}) -> {new_type}")

if updates:
    cur.executemany("UPDATE scan_jobs SET job_type=? WHERE id=?", updates)
    conn.commit()
    print(f"\nTagged {len(updates)} job(s).")
else:
    print("No jobs needed tagging.")

conn.close()
