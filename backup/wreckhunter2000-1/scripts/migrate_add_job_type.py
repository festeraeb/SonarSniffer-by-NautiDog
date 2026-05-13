#!/usr/bin/env python3
"""
DB migration: add job pipeline columns + worker heartbeat table.

New columns on scan_jobs:
  job_type       TEXT DEFAULT 'cpu'     -- cpu | gpu | gpu_tpu
  pipeline_stage TEXT DEFAULT 'process' -- process | postprocess
  parent_id      TEXT                   -- postprocess job links back to GPU parent

New table:
  worker_heartbeats(worker_id PK, has_gpu, has_tpu, vram_gb, gpu_label, last_seen)
"""
import sqlite3, os, sys
from pathlib import Path

DB = Path(__file__).resolve().parent.parent / "db" / "scan_queue.db"

if not DB.exists():
    sys.exit(f"ERROR: {DB} not found")

conn = sqlite3.connect(str(DB))
cur  = conn.cursor()

# ── scan_jobs new columns ─────────────────────────────────────────────────────
existing = {row[1] for row in cur.execute("PRAGMA table_info(scan_jobs)").fetchall()}

for col, defn in [
    ("job_type",       "TEXT DEFAULT 'cpu'"),
    ("pipeline_stage", "TEXT DEFAULT 'process'"),
    ("parent_id",      "TEXT"),
]:
    if col not in existing:
        cur.execute(f"ALTER TABLE scan_jobs ADD COLUMN {col} {defn}")
        print(f"  + scan_jobs.{col}")
    else:
        print(f"  = scan_jobs.{col} (already exists)")

# ── worker_heartbeats table ───────────────────────────────────────────────────
cur.execute("""
    CREATE TABLE IF NOT EXISTS worker_heartbeats (
        worker_id  TEXT PRIMARY KEY,
        has_gpu    INTEGER DEFAULT 0,
        has_tpu    INTEGER DEFAULT 0,
        vram_gb    REAL    DEFAULT 0,
        gpu_label  TEXT    DEFAULT '',
        last_seen  TEXT    NOT NULL
    )
""")
print("  + worker_heartbeats table ready")

conn.commit()
conn.close()
print("Migration complete.")
