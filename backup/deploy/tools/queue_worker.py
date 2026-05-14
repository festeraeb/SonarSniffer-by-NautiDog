#!/usr/bin/env python3
"""
CESAROPS Queue Worker — polls scan_queue.db and dispatches jobs to i7.

Runs on the laptop. Connects to i7 via SSH (Tailscale or jump tunnel),
runs run_job_remote.py for each job, and updates scan_queue.db with results.

Usage:
    python queue_worker.py                   # 2 concurrent workers, poll every 30 s
    python queue_worker.py --workers 3       # 3 concurrent jobs
    python queue_worker.py --dry-run         # log what would run, no DB mutations
    python queue_worker.py --poll 60         # slower poll interval
    python queue_worker.py --once            # claim + run one job then exit
"""

import argparse
import json
import os
import socket
import sqlite3
import sys
import time
from concurrent.futures import Future, ThreadPoolExecutor
from datetime import datetime, timezone
from pathlib import Path
from threading import Lock

from remote_dispatch import (
    SSHNode,
    I7_HOST,
    I7_USER,
    I7_PASS,
    I7_KEY,
    I7_WORK,
    I7_JUMP_HOST,
    I7_JUMP_USER,
)

# ── Constants ────────────────────────────────────────────────────────────────

DB_PATH   = Path(__file__).parent / "db" / "scan_queue.db"
I7_REPO   = "/home/cesarops/wreckhunter2000-1"
WORKER_ID = socket.gethostname()

# Scripts that must be present on i7 before any job runs
_SYNC_FILES = [
    Path(__file__).parent / "run_job_remote.py",
]

_db_lock = Lock()   # serialise all DB writes (SQLite WAL-mode is not always available)

# ── Pi queue-DB sync ──────────────────────────────────────────────────────────
# After every DB mutation we push scan_queue.db to the Pi conductor so the
# /workers and /jobs dashboard endpoints always reflect current state.

_PI_HOST     = "100.127.66.32"
_PI_USER     = "pi"
_PI_PASS     = "admin"
_PI_QUEUE_DB = "/home/pi/wreckhunter2000-1/db/scan_queue.db"


def _push_queue_to_pi() -> None:
    """SFTP scan_queue.db → Pi in a fire-and-forget background thread."""
    def _upload():
        try:
            import paramiko
            t = paramiko.Transport((_PI_HOST, 22))
            t.connect(username=_PI_USER, password=_PI_PASS)
            sftp = paramiko.SFTPClient.from_transport(t)
            sftp.put(str(DB_PATH), _PI_QUEUE_DB)
            sftp.close()
            t.close()
        except Exception as exc:
            print(f"  [SYNC] warn: could not push queue DB to Pi: {exc}", flush=True)

    import threading
    threading.Thread(target=_upload, daemon=True).start()

# ── Idle training — rotated on i7 when queue is empty ────────────────────────
# Commands are run via `nohup … &` so they never block job dispatch.
# The pgrep pattern is tested on i7 to avoid duplicate launches.
IDLE_TRAIN_CMDS = [
    "python wreck_ml_trainer.py",
    "python cesarops_core/drifter_training_pipeline.py",
]
_IDLE_TRAIN_PGREP = "wreck_ml_trainer.py\\|drifter_training_pipeline.py"


# ── Startup sync ──────────────────────────────────────────────────────────────

def sync_scripts_to_i7(dry_run: bool = False) -> bool:
    """
    Upload run_job_remote.py (and any other helper scripts) to i7 via SFTP.
    Returns True on success or if dry_run.
    """
    if dry_run:
        for f in _SYNC_FILES:
            print(f"  [DRY-RUN] would sync {f.name} → i7:{I7_REPO}/", flush=True)
        return True

    try:
        import paramiko
    except ImportError:
        print("[SYNC] paramiko not available — skipping file sync", flush=True)
        return False

    node = SSHNode(
        I7_HOST, I7_USER, I7_PASS, I7_KEY,
        jump_host=I7_JUMP_HOST, jump_user=I7_JUMP_USER, jump_key=I7_KEY,
    )
    try:
        node.connect()
        sftp = node._client.open_sftp()
        for local_path in _SYNC_FILES:
            remote_path = f"{I7_REPO}/{local_path.name}"
            sftp.put(str(local_path), remote_path)
            print(f"  [SYNC] {local_path.name} → i7:{remote_path}", flush=True)
        sftp.close()
        return True
    except Exception as exc:
        print(f"  [SYNC] WARNING: could not sync scripts: {exc}", flush=True)
        return False
    finally:
        node.close()


# ── DB helpers ────────────────────────────────────────────────────────────────

def _open_db() -> sqlite3.Connection:
    conn = sqlite3.connect(str(DB_PATH), timeout=15, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    return conn


def claim_job(conn: sqlite3.Connection) -> dict | None:
    """
    Atomically claim the highest-priority QUEUED job.
    Returns the job row as a plain dict, or None if the queue is empty.
    """
    with _db_lock:
        row = conn.execute(
            "SELECT * FROM scan_jobs "
            "WHERE status='QUEUED' "
            "ORDER BY priority DESC, created_at ASC "
            "LIMIT 1"
        ).fetchone()
        if not row:
            return None

        now = datetime.now(timezone.utc).isoformat()
        affected = conn.execute(
            "UPDATE scan_jobs "
            "SET status='RUNNING', started_at=?, worker_id=? "
            "WHERE id=? AND status='QUEUED'",
            (now, WORKER_ID, row["id"]),
        ).rowcount
        conn.commit()

        if affected == 0:
            return None  # another thread/process claimed it first

        _push_queue_to_pi()
        return dict(row)


def finish_job(
    conn: sqlite3.Connection,
    job_id: str,
    success: bool,
    result_path: str = "",
    error_msg: str = "",
) -> None:
    """Write the terminal state (DONE or FAILED) back to the DB."""
    status = "DONE" if success else "FAILED"
    now    = datetime.now(timezone.utc).isoformat()
    with _db_lock:
        conn.execute(
            "UPDATE scan_jobs "
            "SET status=?, finished_at=?, result_path=?, error_msg=? "
            "WHERE id=?",
            (
                status,
                now,
                result_path if success else None,
                None if success else error_msg[:4000],
                job_id,
            ),
        )
        conn.commit()
    _push_queue_to_pi()


# ── Dispatch helpers ────────────────────────────────────────────────────────

def _build_node() -> SSHNode:
    """Build an SSHNode for i7 (Tailscale-direct or via Pi jump)."""
    return SSHNode(
        I7_HOST, I7_USER, I7_PASS, I7_KEY,
        jump_host=I7_JUMP_HOST,
        jump_user=I7_JUMP_USER,
        jump_key=I7_KEY,
    )


def run_job_on_i7(job: dict, dry_run: bool = False) -> tuple[bool, str, str]:
    """
    SSH to i7 and execute run_job_remote.py for this job.

    Returns:
        (success: bool, result_path: str, error_msg: str)
    """
    job_payload = {
        "id":      job["id"],
        "label":   job["label"],
        "bbox":    json.loads(job["bbox"])    if isinstance(job["bbox"],    str) else job["bbox"],
        "sensors": json.loads(job["sensors"]) if isinstance(job["sensors"], str) else job["sensors"],
        "params":  json.loads(job["params"])  if isinstance(job["params"],  str) else (job["params"] or {}),
    }

    # Shell-safe: escape single quotes (bash ' → '\'' trick)
    job_json = json.dumps(job_payload).replace("'", "'\\''")
    cmd = (
        f"cd {I7_REPO} && "
        f"python run_job_remote.py --job-json '{job_json}' 2>&1"
    )

    if dry_run:
        preview = cmd[:180]
        print(f"  [DRY-RUN] SSH i7: {preview}...", flush=True)
        return True, "dry_run", ""

    node = _build_node()
    try:
        print(f"  [DISPATCH] {job['label']} (id={job['id'][:8]}) → i7", flush=True)
        result = node.run(cmd, timeout=14_400)  # 4-hour hard cap per job

        stdout    = result["stdout"]
        exit_code = result["exit_code"]
        duration  = result["duration_s"]

        if exit_code == 0:
            # Pull result_path from the last JSON line emitted by run_job_remote.py
            result_path = f"{I7_REPO}/outputs/{job['label']}"
            for line in reversed(stdout.splitlines()):
                line = line.strip()
                if line.startswith("{"):
                    try:
                        data = json.loads(line)
                        if "result_path" in data:
                            result_path = data["result_path"]
                        break
                    except json.JSONDecodeError:
                        pass

            print(
                f"  [DONE] {job['label']} in {duration:.0f}s  →  {result_path}",
                flush=True,
            )
            return True, result_path, ""

        else:
            # Grab last 1 kB of combined output as the error message
            err_tail = (result["stderr"] or stdout)[-1000:].strip()
            print(
                f"  [FAILED] {job['label']} exit={exit_code} after {duration:.0f}s\n"
                f"           {err_tail[:200]}",
                flush=True,
            )
            return False, "", err_tail

    except Exception as exc:
        msg = str(exc)
        print(f"  [ERROR] {job['label']}: {msg}", flush=True)
        return False, "", msg

    finally:
        node.close()


def run_job_locally(job: dict, dry_run: bool = False) -> tuple[bool, str, str]:
    """
    Run run_job_remote.py as a local subprocess on the laptop.
    Returns (success, result_path, error_msg).
    """
    import subprocess
    import re as _re

    job_payload = {
        "id":      job["id"],
        "label":   job["label"],
        "bbox":    json.loads(job["bbox"])    if isinstance(job["bbox"],    str) else job["bbox"],
        "sensors": json.loads(job["sensors"]) if isinstance(job["sensors"], str) else job["sensors"],
        "params":  json.loads(job["params"])  if isinstance(job["params"],  str) else (job["params"] or {}),
    }
    job_json = json.dumps(job_payload)

    if dry_run:
        print(f"  [DRY-RUN] LOCAL: run_job_remote.py --job-json '{{...}}' for {job['label']}", flush=True)
        return True, "dry_run", ""

    label = job["label"]
    safe  = _re.sub(r"[^\w\-]", "_", label)
    result_path = str(Path(__file__).parent / "outputs" / safe)

    print(f"  [DISPATCH] {label} (id={job['id'][:8]}) → laptop (local)", flush=True)

    import time as _time
    t0  = _time.time()
    proc = subprocess.run(
        [sys.executable, str(Path(__file__).parent / "run_job_remote.py"), "--job-json", job_json],
        capture_output=True,
        text=True,
    )
    duration = _time.time() - t0
    combined = proc.stdout + proc.stderr

    if proc.returncode == 0:
        for line in reversed(combined.splitlines()):
            line = line.strip()
            if line.startswith("{"):
                try:
                    data = json.loads(line)
                    if "result_path" in data:
                        result_path = data["result_path"]
                    break
                except json.JSONDecodeError:
                    pass
        print(f"  [DONE] {label} in {duration:.0f}s  →  {result_path}", flush=True)
        return True, result_path, ""
    else:
        err_tail = combined[-1000:].strip()
        print(
            f"  [FAILED] {label} exit={proc.returncode} after {duration:.0f}s\n"
            f"           {err_tail[:200]}",
            flush=True,
        )
        return False, "", err_tail


# ── Thread-pool wrapper ───────────────────────────────────────────────────────

def _run_and_capture(job: dict, dry_run: bool, dispatch_fn) -> tuple[dict, bool, str, str]:
    """Wrapper so ThreadPoolExecutor can return (job, success, result_path, error_msg)."""
    success, result_path, error_msg = dispatch_fn(job, dry_run=dry_run)
    return job, success, result_path, error_msg


# ── Main loop ─────────────────────────────────────────────────────────────────

def worker_loop(
    conn: sqlite3.Connection,
    max_workers: int,
    dry_run: bool,
    poll_interval: int,
    dispatch_fn=None,
    min_backlog: int = 0,
) -> None:
    """Continuously poll the queue and run up to max_workers jobs in parallel."""
    if dispatch_fn is None:
        dispatch_fn = run_job_on_i7
    target_label = "laptop (local)" if dispatch_fn is run_job_locally else "i7 (SSH)"
    print(
        f"[WORKER] Started — target={target_label}, max_workers={max_workers}, "
        f"poll={poll_interval}s, min_backlog={min_backlog}, dry_run={dry_run}, worker_id={WORKER_ID}",
        flush=True,
    )

    active: dict[str, Future] = {}  # job_id → Future

    with ThreadPoolExecutor(max_workers=max_workers) as pool:
        while True:
            # ── Harvest completed futures ──────────────────────────────────
            done_ids = [jid for jid, f in active.items() if f.done()]
            for jid in done_ids:
                fut = active.pop(jid)
                try:
                    job, success, result_path, error_msg = fut.result()
                    if not dry_run:
                        finish_job(conn, job["id"], success, result_path, error_msg)
                    tag = "DONE" if success else "FAILED"
                    print(f"  [{tag}] {job['label']} (id={job['id'][:8]})", flush=True)
                except Exception as exc:
                    print(f"  [ERROR] Future raised: {exc}", flush=True)

            # ── Fill free slots with new jobs ──────────────────────────────
            free = max_workers - len(active)
            for _ in range(free):
                # Laptop back-pressure: only claim when queue depth ≥ min_backlog
                if min_backlog > 0:
                    backlog = conn.execute(
                        "SELECT COUNT(*) FROM scan_jobs WHERE status='QUEUED'"
                    ).fetchone()[0]
                    if backlog < min_backlog:
                        break  # not busy enough to warrant laptop help

                if dry_run:
                    # Peek without claiming
                    row = conn.execute(
                        "SELECT * FROM scan_jobs WHERE status='QUEUED' "
                        "ORDER BY priority DESC, created_at ASC LIMIT 1"
                    ).fetchone()
                    job = dict(row) if row else None
                else:
                    job = claim_job(conn)
                if job is None:
                    break
                print(
                    f"  [CLAIM] {job['label']} (id={job['id'][:8]}, priority={job['priority']})",
                    flush=True,
                )
                fut = pool.submit(_run_and_capture, job, dry_run, dispatch_fn)
                active[job["id"]] = fut

            # ── Decide whether to keep looping ────────────────────────────
            queued  = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='QUEUED'").fetchone()[0]
            running = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='RUNNING'").fetchone()[0]

            if queued == 0 and running == 0 and len(active) == 0:
                print("[WORKER] Queue drained — all jobs complete.", flush=True)
                break

            # Sleep briefly when only running jobs remain (no new work yet)
            sleep_s = 5 if (queued == 0 and len(active) > 0) else poll_interval
            time.sleep(sleep_s)


# ── CLI entry ─────────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="CESAROPS Queue Worker")
    parser.add_argument("--target",      choices=["i7", "laptop"], default="i7",
                        help="Where to run jobs: 'i7' (SSH) or 'laptop' (local subprocess). Default: i7")
    parser.add_argument("--workers",     type=int, default=2,
                        help="Max concurrent jobs (default: 2 for i7, 1 for laptop)")
    parser.add_argument("--min-backlog", type=int, default=0,
                        help="Laptop only: minimum QUEUED depth before claiming a job (default: 0=always)")
    parser.add_argument("--dry-run",     action="store_true",
                        help="Show jobs without actually running them or mutating DB")
    parser.add_argument("--poll",        type=int, default=30,
                        help="Queue poll interval in seconds (default: 30)")
    parser.add_argument("--once",        action="store_true",
                        help="Claim and run exactly one job, then exit")
    args = parser.parse_args()

    # Pick dispatch function
    if args.target == "laptop":
        dispatch_fn   = run_job_locally
        default_workers = 1
    else:
        dispatch_fn   = run_job_on_i7
        default_workers = 2
    max_workers = args.workers if args.workers != 2 else default_workers

    if not DB_PATH.exists():
        print(f"ERROR: Queue DB not found: {DB_PATH}", file=sys.stderr)
        return 1

    conn    = _open_db()
    queued  = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='QUEUED'").fetchone()[0]
    running = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='RUNNING'").fetchone()[0]
    done    = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='DONE'").fetchone()[0]
    failed  = conn.execute("SELECT COUNT(*) FROM scan_jobs WHERE status='FAILED'").fetchone()[0]
    print(
        f"[WORKER] Queue snapshot — QUEUED={queued}, RUNNING={running}, "
        f"DONE={done}, FAILED={failed}",
        flush=True,
    )

    if queued == 0 and not args.once:
        print("[WORKER] Nothing queued. Exiting.", flush=True)
        conn.close()
        return 0

    # Sync helper scripts to i7 only when targeting i7
    if args.target == "i7":
        print("[WORKER] Syncing scripts to i7...", flush=True)
        sync_scripts_to_i7(dry_run=args.dry_run)

    if args.once:
        if args.dry_run:
            # Peek only — no DB writes
            row = conn.execute(
                "SELECT * FROM scan_jobs WHERE status='QUEUED' "
                "ORDER BY priority DESC, created_at ASC LIMIT 1"
            ).fetchone()
            if row is None:
                print("[WORKER] No jobs queued.", flush=True)
            else:
                job = dict(row)
                print(f"[WORKER] [DRY-RUN] would run: {job['label']} (id={job['id'][:8]}, priority={job['priority']})", flush=True)
                dispatch_fn(job, dry_run=True)
            conn.close()
            return 0

        job = claim_job(conn)
        if job is None:
            print("[WORKER] No jobs available.", flush=True)
            conn.close()
            return 0
        print(f"[WORKER] --once: running {job['label']} (id={job['id'][:8]})", flush=True)
        success, result_path, error_msg = dispatch_fn(job, dry_run=False)
        finish_job(conn, job["id"], success, result_path, error_msg)
        tag = "DONE" if success else "FAILED"
        print(f"[WORKER] {tag}: {job['label']}", flush=True)
        conn.close()
        return 0 if success else 1

    try:
        worker_loop(conn, max_workers, args.dry_run, args.poll,
                    dispatch_fn=dispatch_fn, min_backlog=args.min_backlog)
    except KeyboardInterrupt:
        print("\n[WORKER] Interrupted by user. Active jobs will be retried on next run.", flush=True)
        if not args.dry_run:
            # Reset any RUNNING jobs we claimed so they rejoin the queue on restart
            with _db_lock:
                conn.execute(
                    "UPDATE scan_jobs SET status='QUEUED', started_at=NULL, worker_id=NULL "
                    "WHERE status='RUNNING' AND worker_id=?",
                    (WORKER_ID,),
                )
                conn.commit()
    finally:
        conn.close()

    return 0


if __name__ == "__main__":
    sys.exit(main())
