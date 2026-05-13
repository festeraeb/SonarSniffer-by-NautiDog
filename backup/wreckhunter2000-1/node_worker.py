#!/usr/bin/env python3
"""
CESAROPS Node Worker — autonomous LAN-native job runner.

Runs on ANY node in the house (laptop, i7, incoming machines).
Connects to the Pi conductor via HTTP (same LAN, no SSH needed).
Processes jobs it is capable of; trains on idle; announces its capabilities.

Usage:
    python node_worker.py                  # run forever, all capable jobs
    python node_worker.py --workers 2      # 2 concurrent jobs
    python node_worker.py --min-backlog 3  # only help when 3+ jobs queued (laptop)
    python node_worker.py --dry-run        # preview only, no DB mutations
    python node_worker.py --once           # claim and run one job then exit

Node capabilities are read from environment / .env:
    NODE_HAS_GPU=true           # does this machine have a GPU?
    NODE_GPU=QuadroM2200        # human label (for dashboard)
    NODE_HAS_TPU=false          # connected to a TPU server?
    NODE_VRAM_GB=4              # usable VRAM in GB
    NODE_TRAIN_IDLE=true        # run training commands when queue is empty
    PI_API_URL=http://10.0.0.226:8099   # Pi conductor REST API

Job params that control which nodes pick up a job:
    requires_gpu:   true/false  (default false — any node)
    requires_tpu:   true/false  (default false)
    min_vram_gb:    float       (default 0)
"""

import argparse
import json
import os
import re
import socket
import subprocess
import sys
import threading
import time
import urllib.error
import urllib.request
from concurrent.futures import Future, ThreadPoolExecutor
from pathlib import Path

# ── Load .env ────────────────────────────────────────────────────────────────

_REPO = Path(__file__).resolve().parent
_dotenv: dict[str, str] = {}
_env_file = _REPO / ".env"
if _env_file.exists():
    for _line in _env_file.read_text().splitlines():
        _line = _line.strip()
        if _line and not _line.startswith("#") and "=" in _line:
            _k, _, _v = _line.partition("=")
            _dotenv[_k.strip()] = _v.strip().strip('"').strip("'")


def _env(key: str, default: str = "") -> str:
    return os.environ.get(key, _dotenv.get(key, default))


def _env_bool(key: str, default: bool = False) -> bool:
    v = _env(key, "true" if default else "false").lower()
    return v in ("1", "true", "yes", "on")


# ── Node identity + capabilities ─────────────────────────────────────────────

WORKER_ID   = socket.gethostname()
HAS_GPU     = _env_bool("NODE_HAS_GPU", False)
HAS_TPU     = _env_bool("NODE_HAS_TPU", False)
VRAM_GB     = float(_env("NODE_VRAM_GB", "0") or "0")
GPU_LABEL   = _env("NODE_GPU", "")
TRAIN_IDLE  = _env_bool("NODE_TRAIN_IDLE", True)

# PI_API is resolved at worker_loop() start.
# If PI_API_URL is set in .env, that wins.
# Otherwise the worker probes LAN → Tailscale at startup.
_PI_API_OVERRIDE = _env("PI_API_URL", "").rstrip("/")
PI_API = _PI_API_OVERRIDE or "http://10.0.0.226:8099"   # default; may be updated


def _probe_url(url: str, timeout: int = 3) -> bool:
    """Return True if url/health responds 200."""
    try:
        return urllib.request.urlopen(f"{url.rstrip('/')}/health", timeout=timeout).status == 200
    except Exception:
        return False


def _auto_discover_pi() -> str:
    """
    If no explicit PI_API_URL is configured, probes candidate addresses and
    returns the first reachable one.  Candidates (in order):
      1. LAN direct   10.0.0.226:8099
      2. Tailscale    100.127.66.32:8099
    """
    if _PI_API_OVERRIDE:
        return _PI_API_OVERRIDE
    for candidate in ("http://10.0.0.226:8099", "http://100.127.66.32:8099"):
        if _probe_url(candidate):
            return candidate
    return "http://10.0.0.226:8099"  # best guess if neither responds yet

# Idle training commands (run locally when queue is empty)
IDLE_TRAIN_CMDS = [
    [sys.executable, str(_REPO / "wreck_ml_trainer.py")],
    [sys.executable, str(_REPO / "cesarops_core" / "drifter_training_pipeline.py")],
]

# ── HTTP helpers ─────────────────────────────────────────────────────────────

def _api(method: str, path: str, body: dict | None = None, timeout: int = 15) -> dict:
    url = f"{PI_API}{path}"
    data = json.dumps(body).encode() if body else None
    req = urllib.request.Request(
        url, data=data, method=method,
        headers={"Content-Type": "application/json"} if data else {},
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as r:
            return json.loads(r.read())
    except urllib.error.HTTPError as e:
        raise RuntimeError(f"HTTP {e.code} from {path}: {e.read().decode()[:200]}")
    except Exception as e:
        raise RuntimeError(f"API error {path}: {e}")


def _claim_job(role: str = "all") -> dict | None:
    """Ask the Pi to assign us a suitable QUEUED job. Returns job dict or None."""
    resp = _api("POST", "/jobs/claim", {
        "worker_id": WORKER_ID,
        "has_gpu":   HAS_GPU,
        "has_tpu":   HAS_TPU,
        "vram_gb":   VRAM_GB,
        "role":      role,
    })
    return resp.get("job")


def _finish_job(job_id: str, success: bool, result_path: str = "", error_msg: str = "") -> None:
    _api("POST", f"/jobs/{job_id}/finish", {
        "success":     success,
        "result_path": result_path,
        "error_msg":   error_msg,
    })


def _queue_summary() -> dict:
    """Quick status check — returns {"QUEUED":N, "RUNNING":N, ...}."""
    try:
        resp = _api("GET", "/workers", timeout=8)
        return resp.get("summary", {})
    except Exception:
        return {}


def _get_online_workers() -> list:
    """Return list of workers seen by Pi within the last 90 s."""
    try:
        return _api("GET", "/workers/online", timeout=8).get("workers", [])
    except Exception:
        return []


def _cpu_only_workers_online() -> list:
    """Return online workers that have no GPU (pure CPU nodes)."""
    return [w for w in _get_online_workers() if not w.get("has_gpu", False)]


def _submit_postprocess_job(parent_job: dict, gpu_result_path: str) -> str:
    """
    Submit a new 'cpu' postprocess job derived from a finished gpu_tpu job.
    Returns the new job id.
    """
    params = dict(parent_job.get("params") or {})
    params["pipeline_stage"]  = "postprocess"
    params["source_result"]   = gpu_result_path
    try:
        resp = _api("POST", "/jobs", {
            "label":          f"{parent_job['label']}_postprocess",
            "bbox":           parent_job.get("bbox", []),
            "sensors":        parent_job.get("sensors", []),
            "params":         params,
            "priority":       (parent_job.get("priority") or 1) + 1,
            "job_type":       "cpu",
            "pipeline_stage": "postprocess",
            "parent_id":      parent_job.get("id", ""),
        })
        new_id = resp.get("id", "?")
        print(f"  [PIPELINE] Postprocess job queued: id={new_id}", flush=True)
        return new_id
    except Exception as e:
        print(f"  [PIPELINE] WARNING: could not queue postprocess job: {e}", flush=True)
        return ""


# ── Heartbeat ────────────────────────────────────────────────────────────────────────

def _heartbeat_loop(interval: int = 30) -> None:
    """Background thread — announce this worker to the Pi every `interval` s."""
    while True:
        try:
            _api("POST", "/workers/heartbeat", {
                "worker_id": WORKER_ID,
                "has_gpu":   HAS_GPU,
                "has_tpu":   HAS_TPU,
                "vram_gb":   VRAM_GB,
                "gpu_label": GPU_LABEL,
            }, timeout=8)
        except Exception as e:
            print(f"  [HEARTBEAT] warn: {e}", flush=True)
        time.sleep(interval)


# ── Job execution ─────────────────────────────────────────────────────────────

def _run_remote_subprocess(job: dict, dry_run: bool, phase: str = "process") -> tuple[bool, str, str]:
    """
    Core worker: run run_job_remote.py as a subprocess.
    `phase` is injected into params so the runner can adapt its pipeline.
    Returns (success, result_path, error_msg).
    """
    label = job.get("label", "unknown")
    safe  = re.sub(r"[^\w\-]", "_", label)
    result_path = str(_REPO / "outputs" / safe)

    params = job.get("params", {})
    if isinstance(params, str):
        try:
            params = json.loads(params)
        except Exception:
            params = {}

    payload = {
        "id":      job["id"],
        "label":   label,
        "bbox":    job["bbox"]    if isinstance(job["bbox"],    list) else json.loads(job["bbox"] or "[]"),
        "sensors": job["sensors"] if isinstance(job["sensors"], list) else json.loads(job["sensors"] or "[]"),
        "params":  {**params, "pipeline_stage": phase},
    }

    if dry_run:
        print(f"  [DRY-RUN] {phase}: {label} (id={job['id'][:8]})", flush=True)
        return True, "dry_run", ""

    print(f"  [RUN:{phase}] {label} (id={job['id'][:8]}) on {WORKER_ID}", flush=True)
    t0 = time.time()
    try:
        proc = subprocess.run(
            [sys.executable, str(_REPO / "run_job_remote.py"),
             "--job-json", json.dumps(payload)],
            capture_output=True, text=True,
            timeout=4 * 3600,  # 4-hour hard cap
        )
        duration = time.time() - t0
        combined = proc.stdout + proc.stderr

        if proc.returncode == 0:
            for line in reversed(combined.splitlines()):
                line = line.strip()
                if line.startswith("{"):
                    try:
                        d = json.loads(line)
                        if "result_path" in d:
                            result_path = d["result_path"]
                        break
                    except json.JSONDecodeError:
                        pass
            print(f"  [DONE:{phase}] {label} ({duration:.0f}s) → {result_path}", flush=True)
            return True, result_path, ""
        else:
            err = combined[-1000:].strip()
            print(f"  [FAIL:{phase}] {label} exit={proc.returncode} ({duration:.0f}s): {err[:120]}", flush=True)
            return False, "", err
    except subprocess.TimeoutExpired:
        msg = "Timeout after 4h"
        print(f"  [FAIL:{phase}] {label}: {msg}", flush=True)
        return False, "", msg
    except Exception as exc:
        msg = str(exc)
        print(f"  [ERROR:{phase}] {label}: {msg}", flush=True)
        return False, "", msg


def run_job(job: dict, dry_run: bool = False) -> tuple[bool, str, str]:
    """
    Dispatch job to the correct execution pipeline based on job_type.

    job_type  | pipeline_stage | action
    ----------+----------------+---------------------------------------
    cpu / gpu | process        | run_job_remote.py directly
    gpu_tpu   | process        | GPU+TPU pass → route postprocess
    any       | postprocess    | run postprocess phase (CPU work)
    """
    job_type       = (job.get("job_type") or "cpu").lower()
    pipeline_stage = (job.get("pipeline_stage") or "process").lower()

    # ── Postprocess stage (handed off or claimed by CPU node) ─────────────────
    if pipeline_stage == "postprocess":
        return _run_remote_subprocess(job, dry_run, phase="postprocess")

    # ── GPU + TPU two-phase pipeline ──────────────────────────────────────────
    if job_type == "gpu_tpu" and not dry_run:
        label = job.get("label", "unknown")
        gpu_params = {**(job.get("params") or {}), "tpu_enabled": True}
        success, result_path, error_msg = _run_remote_subprocess(
            {**job, "params": gpu_params}, dry_run, phase="process"
        )
        if not success:
            return False, result_path, error_msg

        cpu_workers = _cpu_only_workers_online()
        if cpu_workers:
            print(
                f"  [PIPELINE] {label}: "
                f"{len(cpu_workers)} CPU-only worker(s) online — queuing postprocess job",
                flush=True,
            )
            _submit_postprocess_job(job, result_path)
        else:
            print(
                f"  [PIPELINE] {label}: no CPU-only workers — running postprocess locally",
                flush=True,
            )
            _run_remote_subprocess(
                {**job, "params": {**(job.get("params") or {}), "source_result": result_path}},
                dry_run, phase="postprocess",
            )
        return True, result_path, ""

    # ── cpu / gpu: straight subprocess ────────────────────────────────────────
    return _run_remote_subprocess(job, dry_run, phase=pipeline_stage)


# ── Idle training ─────────────────────────────────────────────────────────────

_idle_proc: subprocess.Popen | None = None


def _maybe_start_training() -> None:
    global _idle_proc
    if not TRAIN_IDLE:
        return
    if _idle_proc is not None and _idle_proc.poll() is None:
        return  # already running
    for cmd in IDLE_TRAIN_CMDS:
        if Path(cmd[1]).exists():
            print(f"  [TRAIN] Starting idle trainer: {Path(cmd[1]).name}", flush=True)
            _idle_proc = subprocess.Popen(cmd, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            return


def _stop_training() -> None:
    global _idle_proc
    if _idle_proc is not None and _idle_proc.poll() is None:
        print("  [TRAIN] Stopping idle trainer (job incoming)", flush=True)
        _idle_proc.terminate()
        try:
            _idle_proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            _idle_proc.kill()
    _idle_proc = None


# ── Main worker loop ──────────────────────────────────────────────────────────

def worker_loop(max_workers: int, dry_run: bool, poll: int, min_backlog: int, run_once: bool) -> None:
    global PI_API
    # Resolve Pi URL (auto-discover if no override in .env)
    PI_API = _auto_discover_pi()

    caps = f"gpu={'yes' if HAS_GPU else 'no'}"
    if GPU_LABEL:
        caps += f"({GPU_LABEL})"
    caps += f" tpu={'yes' if HAS_TPU else 'no'}"
    caps += f" vram={VRAM_GB}GB"

    print(
        f"[NODE] {WORKER_ID} — {caps} | pi={PI_API} | workers={max_workers} | "
        f"min_backlog={min_backlog} | train_idle={TRAIN_IDLE}",
        flush=True,
    )

    # Start heartbeat thread so Pi knows this worker is online
    hb_thread = threading.Thread(target=_heartbeat_loop, daemon=True, name="heartbeat")
    hb_thread.start()
    print("[NODE] Heartbeat thread started (30 s interval)", flush=True)

    # Quick connectivity check
    try:
        s = _queue_summary()
        q = s.get("QUEUED", "?")
        r = s.get("RUNNING", "?")
        print(f"[NODE] Pi reachable — QUEUED={q}, RUNNING={r}", flush=True)
    except Exception as e:
        print(f"[NODE] WARNING: cannot reach Pi API ({e}) — will keep retrying", flush=True)

    active: dict[str, Future] = {}

    with ThreadPoolExecutor(max_workers=max_workers) as pool:
        while True:
            # ── Harvest completed futures ─────────────────────────────────
            done_ids = [jid for jid, f in active.items() if f.done()]
            for jid in done_ids:
                fut = active.pop(jid)
                try:
                    job, success, rpath, errmsg = fut.result()
                    if not dry_run:
                        _finish_job(jid, success, rpath, errmsg)
                    tag = "DONE" if success else "FAILED"
                    print(f"  [{tag}] {job['label']} (id={jid[:8]})", flush=True)
                except Exception as exc:
                    print(f"  [ERROR] future raised: {exc}", flush=True)

            # ── Fill free slots ───────────────────────────────────────────
            free = max_workers - len(active)
            claimed_this_tick = 0

            for _ in range(free):
                # Back-pressure: laptop only chips in when queue is deep enough
                if min_backlog > 0:
                    try:
                        summary = _queue_summary()
                        if summary.get("QUEUED", 0) < min_backlog:
                            break
                    except Exception:
                        break

                try:
                    job = None if dry_run else _claim_job()
                except Exception as e:
                    print(f"  [WARN] claim failed: {e}", flush=True)
                    break

                if job is None:
                    break

                claimed_this_tick += 1
                _stop_training()
                print(f"  [CLAIM] {job['label']} (id={job['id'][:8]}, priority={job.get('priority',1)})", flush=True)
                fut = pool.submit(
                    lambda j=job: (j,) + run_job(j, dry_run=dry_run)
                )
                active[job["id"]] = fut

                if run_once:
                    # Wait for that one job to finish
                    pool.shutdown(wait=True)
                    return

            # ── Idle training when nothing is running ─────────────────────
            if len(active) == 0 and claimed_this_tick == 0:
                _maybe_start_training()

            time.sleep(poll)


# ── CLI ───────────────────────────────────────────────────────────────────────

def main() -> int:
    parser = argparse.ArgumentParser(description="CESAROPS Node Worker")
    parser.add_argument("--workers",     type=int, default=1,
                        help="Max concurrent jobs (default: 1)")
    parser.add_argument("--min-backlog", type=int, default=0,
                        help="Only claim jobs when at least N are QUEUED (laptop back-pressure)")
    parser.add_argument("--poll",        type=int, default=15,
                        help="Queue poll interval in seconds (default: 15)")
    parser.add_argument("--dry-run",     action="store_true",
                        help="Show what would run without executing")
    parser.add_argument("--once",        action="store_true",
                        help="Claim and run one job then exit")
    args = parser.parse_args()

    worker_loop(
        max_workers=args.workers,
        dry_run=args.dry_run,
        poll=args.poll,
        min_backlog=args.min_backlog,
        run_once=args.once,
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
