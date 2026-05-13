#!/usr/bin/env python3
"""
CESAROPS Scan Job Queue
=======================
SQLite-backed priority job queue for tile scanning.

Priorities:
  0 = USER_REQUEST  — interrupts everything
  1 = DIRECTED      — named target probe (from known_wrecks / SAR event)
  2 = IDLE          — systematic coverage learning

Usage (CLI):
    python scan_queue.py list
    python scan_queue.py push --label "Andaste" --bbox 42.8,-86.6,43.1,-86.3 --sensors thermal,sar --priority 1
    python scan_queue.py cancel <job_id>
    python scan_queue.py clear-done
"""

import argparse
import json
import sqlite3
import sys
import uuid
from datetime import datetime, timezone
from pathlib import Path
from typing import Dict, List, Optional

QUEUE_DB = Path(__file__).parent / "db" / "scan_queue.db"

PRIORITY_USER     = 0   # Interrupt everything
PRIORITY_DIRECTED = 1   # Named target / SAR event
PRIORITY_IDLE     = 2   # Background coverage learning

STATUS_QUEUED    = "QUEUED"
STATUS_RUNNING   = "RUNNING"
STATUS_DONE      = "DONE"
STATUS_FAILED    = "FAILED"
STATUS_CANCELLED = "CANCELLED"


# ── DB helpers ────────────────────────────────────────────────────────────────

def _get_conn() -> sqlite3.Connection:
    QUEUE_DB.parent.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(str(QUEUE_DB), timeout=15, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    return conn


def init_db():
    """Create table if it doesn't exist."""
    with _get_conn() as c:
        c.execute("""
            CREATE TABLE IF NOT EXISTS scan_jobs (
                id           TEXT PRIMARY KEY,
                priority     INTEGER NOT NULL DEFAULT 2,
                status       TEXT NOT NULL DEFAULT 'QUEUED',
                label        TEXT,
                bbox         TEXT NOT NULL,   -- JSON [lat_min, lon_min, lat_max, lon_max]
                sensors      TEXT NOT NULL,   -- JSON list e.g. ["thermal","sar"]
                params       TEXT,            -- JSON extra params
                created_at   TEXT NOT NULL,
                started_at   TEXT,
                finished_at  TEXT,
                result_path  TEXT,
                uploaded     INTEGER DEFAULT 0,
                error_msg    TEXT,
                worker_id    TEXT
            )
        """)
        c.execute("""
            CREATE INDEX IF NOT EXISTS idx_queue_priority
            ON scan_jobs (status, priority, created_at)
        """)


# ── Public API ────────────────────────────────────────────────────────────────

def push(label: str, bbox: list, sensors: list,
         priority: int = PRIORITY_IDLE, params: dict = None) -> str:
    """
    Add a job to the queue. Returns the job ID.
    bbox = [lat_min, lon_min, lat_max, lon_max]
    sensors = ["thermal", "sar", "optical", "triple_lock", ...]
    """
    init_db()
    job_id = str(uuid.uuid4())[:8]
    now = datetime.now(timezone.utc).isoformat()
    with _get_conn() as c:
        c.execute("""
            INSERT INTO scan_jobs
              (id, priority, status, label, bbox, sensors, params, created_at)
            VALUES (?, ?, 'QUEUED', ?, ?, ?, ?, ?)
        """, (job_id, priority, label,
              json.dumps(bbox), json.dumps(sensors),
              json.dumps(params or {}), now))
    return job_id


def pop_next(worker_id: str) -> Optional[Dict]:
    """
    Atomically claim the highest-priority QUEUED job.
    Returns None if queue is empty.
    """
    init_db()
    conn = _get_conn()
    try:
        # Serialize access: SQLite BEGIN IMMEDIATE
        conn.execute("BEGIN IMMEDIATE")
        row = conn.execute("""
            SELECT * FROM scan_jobs
            WHERE status = 'QUEUED'
            ORDER BY priority ASC, created_at ASC
            LIMIT 1
        """).fetchone()
        if not row:
            conn.rollback()
            return None
        now = datetime.now(timezone.utc).isoformat()
        conn.execute("""
            UPDATE scan_jobs
            SET status='RUNNING', started_at=?, worker_id=?
            WHERE id=?
        """, (now, worker_id, row["id"]))
        conn.commit()
        return dict(row)
    except Exception:
        conn.rollback()
        raise
    finally:
        conn.close()


def has_urgent(current_priority: int) -> bool:
    """Return True if a higher-priority job is waiting (should interrupt)."""
    init_db()
    with _get_conn() as c:
        r = c.execute("""
            SELECT COUNT(*) FROM scan_jobs
            WHERE status='QUEUED' AND priority < ?
        """, (current_priority,)).fetchone()
        return r[0] > 0


def re_queue(job_id: str):
    """Put a RUNNING job back to QUEUED (interrupted by higher priority)."""
    with _get_conn() as c:
        c.execute("""
            UPDATE scan_jobs
            SET status='QUEUED', started_at=NULL, worker_id=NULL
            WHERE id=?
        """, (job_id,))


def mark_done(job_id: str, result_path: str = None):
    with _get_conn() as c:
        c.execute("""
            UPDATE scan_jobs
            SET status='DONE', finished_at=?, result_path=?
            WHERE id=?
        """, (datetime.now(timezone.utc).isoformat(), result_path, job_id))


def mark_failed(job_id: str, error: str):
    with _get_conn() as c:
        c.execute("""
            UPDATE scan_jobs
            SET status='FAILED', finished_at=?, error_msg=?
            WHERE id=?
        """, (datetime.now(timezone.utc).isoformat(), str(error)[:2000], job_id))


def mark_uploaded(job_id: str):
    with _get_conn() as c:
        c.execute("UPDATE scan_jobs SET uploaded=1 WHERE id=?", (job_id,))


def cancel(job_id: str) -> bool:
    """Cancel a QUEUED job. Returns True if it was queued."""
    with _get_conn() as c:
        n = c.execute("""
            UPDATE scan_jobs SET status='CANCELLED'
            WHERE id=? AND status='QUEUED'
        """, (job_id,)).rowcount
        return n > 0


def list_jobs(limit: int = 30, status_filter: str = None) -> List[Dict]:
    init_db()
    with _get_conn() as c:
        if status_filter:
            rows = c.execute("""
                SELECT id, priority, status, label, bbox, sensors, created_at,
                       started_at, finished_at, result_path, uploaded, error_msg
                FROM scan_jobs WHERE status=?
                ORDER BY priority ASC, created_at DESC LIMIT ?
            """, (status_filter, limit)).fetchall()
        else:
            rows = c.execute("""
                SELECT id, priority, status, label, bbox, sensors, created_at,
                       started_at, finished_at, result_path, uploaded, error_msg
                FROM scan_jobs
                ORDER BY priority ASC, created_at DESC LIMIT ?
            """, (limit,)).fetchall()
        return [dict(r) for r in rows]


def queue_depth() -> Dict[str, int]:
    """Return counts by status."""
    init_db()
    with _get_conn() as c:
        rows = c.execute("""
            SELECT status, COUNT(*) as n FROM scan_jobs GROUP BY status
        """).fetchall()
        return {r["status"]: r["n"] for r in rows}


# ── Idle tile grid (Great Lakes systematic coverage) ─────────────────────────

def _generate_coverage_grid(step_deg: float = 0.5) -> List[Dict]:
    """
    Generates a systematic coverage grid for all five Great Lakes.
    Returns list of {lake, bbox} dicts.
    """
    lakes = [
        ("michigan",  41.5, -88.0, 46.2, -84.5),
        ("erie",      41.3, -83.5, 42.9, -79.0),
        ("huron",     43.0, -84.5, 46.5, -79.5),
        ("superior",  46.4, -92.2, 49.0, -84.0),
        ("ontario",   43.2, -79.9, 44.3, -76.0),
    ]
    tiles = []
    for lake, lat0, lon0, lat1, lon1 in lakes:
        lat = lat0
        while lat < lat1:
            lon = lon0
            while lon < lon1:
                tiles.append({
                    "lake": lake,
                    "bbox": [round(lat, 3), round(lon, 3),
                             round(lat + step_deg, 3), round(lon + step_deg, 3)],
                })
                lon = round(lon + step_deg, 3)
            lat = round(lat + step_deg, 3)
    return tiles


COVERAGE_GRID = _generate_coverage_grid()


def next_idle_tile() -> Optional[Dict]:
    """
    Pick the next coverage tile that hasn't been scanned (or was scanned longest ago).
    Checks scan_jobs history to avoid re-doing recent tiles.
    """
    init_db()
    with _get_conn() as c:
        # Get tiles already done (via label matching)
        done_labels = set(
            r[0] for r in c.execute(
                "SELECT label FROM scan_jobs WHERE status IN ('DONE','RUNNING')"
            ).fetchall() if r[0]
        )
    for tile in COVERAGE_GRID:
        label = f"idle:{tile['lake']}:{tile['bbox'][0]},{tile['bbox'][1]}"
        if label not in done_labels:
            return {"label": label, "bbox": tile["bbox"],
                    "lake": tile["lake"],
                    "sensors": ["optical", "thermal"]}
    # All tiles done once — start cycling from the beginning
    tile = COVERAGE_GRID[0]
    return {"label": f"idle:{tile['lake']}:{tile['bbox'][0]},{tile['bbox'][1]}",
            "bbox": tile["bbox"], "lake": tile["lake"],
            "sensors": ["optical", "thermal"]}


# ── CLI ───────────────────────────────────────────────────────────────────────

def _priority_label(p: int) -> str:
    return {0: "USER", 1: "DIRECTED", 2: "IDLE"}.get(p, str(p))


def _print_jobs(jobs: List[Dict]):
    if not jobs:
        print("  (empty)")
        return
    header = f"{'ID':<10} {'PRI':<9} {'STATUS':<12} {'LABEL':<35} {'CREATED':<22}"
    print(header)
    print("-" * len(header))
    for j in jobs:
        pri = _priority_label(j["priority"])
        label = (j["label"] or "")[:34]
        created = (j["created_at"] or "")[:19]
        status = j["status"]
        flag = " ⬆ UPLOADED" if j.get("uploaded") else ""
        err = f"  ERR:{j['error_msg'][:40]}" if j.get("error_msg") else ""
        print(f"{j['id']:<10} {pri:<9} {status:<12} {label:<35} {created}{flag}{err}")


def main():
    p = argparse.ArgumentParser(description="Scan job queue manager")
    sub = p.add_subparsers(dest="cmd")

    # list
    sub.add_parser("list", help="List recent jobs")
    lq = sub.add_parser("queued", help="Show only queued jobs")

    # push
    pp = sub.add_parser("push", help="Add a job")
    pp.add_argument("--label", required=True)
    pp.add_argument("--bbox", required=True, help="lat_min,lon_min,lat_max,lon_max")
    pp.add_argument("--sensors", default="optical,thermal",
                    help="comma-separated sensor list")
    pp.add_argument("--priority", type=int, default=PRIORITY_DIRECTED,
                    choices=[0, 1, 2],
                    help="0=user(urgent) 1=directed 2=idle")

    # cancel
    cp = sub.add_parser("cancel", help="Cancel a queued job")
    cp.add_argument("job_id")

    # clear
    sub.add_parser("clear-done", help="Remove DONE/FAILED/CANCELLED jobs")
    sub.add_parser("depth", help="Show queue depth by status")
    sub.add_parser("next-idle", help="Show what the next idle tile would be")

    args = p.parse_args()

    if args.cmd == "list":
        _print_jobs(list_jobs(30))
    elif args.cmd == "queued":
        _print_jobs(list_jobs(30, "QUEUED"))
    elif args.cmd == "depth":
        d = queue_depth()
        for s, n in sorted(d.items()):
            print(f"  {s:<12} {n}")
    elif args.cmd == "push":
        bbox = [float(x) for x in args.bbox.split(",")]
        sensors = [s.strip() for s in args.sensors.split(",")]
        jid = push(args.label, bbox, sensors, args.priority)
        print(f"Queued job {jid}  (priority={_priority_label(args.priority)})")
    elif args.cmd == "cancel":
        if cancel(args.job_id):
            print(f"Cancelled {args.job_id}")
        else:
            print(f"Job {args.job_id} not found or not in QUEUED state")
    elif args.cmd == "clear-done":
        with _get_conn() as c:
            n = c.execute("""
                DELETE FROM scan_jobs
                WHERE status IN ('DONE','FAILED','CANCELLED')
            """).rowcount
        print(f"Removed {n} finished jobs")
    elif args.cmd == "next-idle":
        tile = next_idle_tile()
        if tile:
            print(json.dumps(tile, indent=2))
        else:
            print("No idle tile available")
    else:
        p.print_help()


if __name__ == "__main__":
    main()
