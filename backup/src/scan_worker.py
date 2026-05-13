#!/usr/bin/env python3
"""
CESAROPS Scan Worker Daemon
============================
Runs continuously on the home machines. When a scan is requested it runs it.
When idle it picks the next unprobed coverage tile and learns.

Features:
  - Priority interrupt: user jobs (priority 0) preempt everything
  - Systematic Great Lakes coverage when truly idle
  - Results uploaded to IONOS SFTP after each scan
  - Band learning: records per-tile spectral hit scores
  - Graceful SIGTERM/SIGINT (finishes current tile segment before stopping)

Usage:
    python scan_worker.py                         # run daemon
    python scan_worker.py --once                  # run one job then exit
    python scan_worker.py --status                # show current state
    python scan_worker.py --pause / --resume      # pause/resume idle scanning

Systemd:
    ExecStart=/home/cesarops/tpu-venv/bin/python scan_worker.py
    Restart=always
    RestartSec=10

Environment variables (via .env):
    IONOS_SFTP_HOST   — sftp hostname (e.g. access.your-server.com)
    IONOS_SFTP_USER   — sftp username
    IONOS_SFTP_PASS   — sftp password (or leave blank to use key auth)
    IONOS_SFTP_PATH   — remote base path (e.g. /wreckhunter/scans)
    WORKER_POLL_SECS  — seconds between queue checks (default 30)
    WORKER_ID         — override worker name (default: hostname)
"""

import argparse
import json
import logging
import os
import shutil
import signal
import socket
import subprocess
import sys
import threading
import time
from datetime import datetime, timezone
from pathlib import Path
from typing import Optional

try:
    import psutil
    _PSUTIL = True
except ImportError:
    _PSUTIL = False

import scan_queue as Q

# ── Setup ─────────────────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent
LOG_FILE = REPO / "logs" / "scan_worker.log"
STATE_FILE = REPO / "db" / "worker_state.json"

LOG_FILE.parent.mkdir(parents=True, exist_ok=True)

logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(message)s",
    handlers=[
        logging.StreamHandler(sys.stdout),
        logging.FileHandler(str(LOG_FILE), encoding="utf-8"),
    ]
)
log = logging.getLogger("scan_worker")


# ── Config ─────────────────────────────────────────────────────────────────────

def _load_env() -> dict:
    env = {}
    p = REPO / ".env"
    if p.exists():
        for line in p.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                env[k.strip()] = v.strip()
    return env

_DOTENV = _load_env()

def _cfg(key: str, default: str = "") -> str:
    return os.environ.get(key, _DOTENV.get(key, default))

WORKER_ID        = _cfg("WORKER_ID", socket.gethostname())
POLL_SECS        = int(_cfg("WORKER_POLL_SECS", "30"))
# OOM guards
MIN_FREE_RAM_GB  = float(_cfg("WORKER_MIN_FREE_RAM_GB", "2.0"))   # won't start a job below this
MAX_JOB_RAM_GB   = float(_cfg("WORKER_MAX_JOB_RAM_GB",  "8.0"))   # kill subprocess if it exceeds this
MEM_POLL_SECS    = float(_cfg("WORKER_MEM_POLL_SECS",   "5.0"))   # how often to check subprocess RSS

IONOS_HOST  = _cfg("IONOS_SFTP_HOST")
IONOS_USER  = _cfg("IONOS_SFTP_USER")
IONOS_PASS  = _cfg("IONOS_SFTP_PASS")
IONOS_PATH  = _cfg("IONOS_SFTP_PATH", "/wreckhunter/scans")


# ── Graceful shutdown ─────────────────────────────────────────────────────────

_STOP     = False
_PAUSE    = False
_CURRENT_JOB: Optional[dict] = None

def _sig_handler(signum, frame):
    global _STOP
    log.info(f"Signal {signum} received — finishing current job then stopping")
    _STOP = True

signal.signal(signal.SIGTERM, _sig_handler)
signal.signal(signal.SIGINT,  _sig_handler)


# ── State file ────────────────────────────────────────────────────────────────

def _write_state(state: dict):
    STATE_FILE.parent.mkdir(parents=True, exist_ok=True)
    STATE_FILE.write_text(json.dumps(state, indent=2))


def _read_state() -> dict:
    if STATE_FILE.exists():
        try:
            return json.loads(STATE_FILE.read_text())
        except Exception:
            pass
    return {}


# ── OOM helpers ──────────────────────────────────────────────────────────────

def _free_ram_gb() -> float:
    """Return available system RAM in GB. Falls back to a large value if psutil absent."""
    if not _PSUTIL:
        return 999.0
    return psutil.virtual_memory().available / (1024 ** 3)


def _check_ram_ok(label: str = "") -> bool:
    """Return False and log a warning when free RAM is below the minimum threshold."""
    free = _free_ram_gb()
    if free < MIN_FREE_RAM_GB:
        log.warning(
            f"OOM guard: only {free:.1f}GB free (min={MIN_FREE_RAM_GB}GB) — "
            f"deferring job {label!r}"
        )
        return False
    return True


def _watchdog_thread(proc: subprocess.Popen, limit_gb: float,
                     stop_event: threading.Event):
    """
    Background thread: monitors the RSS of a subprocess and kills it if the
    process (+ its children) exceeds *limit_gb* GB.
    Exits cleanly when stop_event is set or the process ends.
    """
    if not _PSUTIL:
        return
    limit_bytes = limit_gb * (1024 ** 3)
    try:
        parent = psutil.Process(proc.pid)
    except psutil.NoSuchProcess:
        return

    while not stop_event.is_set():
        try:
            children = parent.children(recursive=True)
            total = parent.memory_info().rss
            for c in children:
                try:
                    total += c.memory_info().rss
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    pass

            if total > limit_bytes:
                log.error(
                    f"OOM watchdog: process {proc.pid} using "
                    f"{total / (1024**3):.1f}GB (limit={limit_gb}GB) — killing"
                )
                try:
                    parent.kill()
                    for c in children:
                        try:
                            c.kill()
                        except (psutil.NoSuchProcess, psutil.AccessDenied):
                            pass
                except (psutil.NoSuchProcess, psutil.AccessDenied):
                    pass
                break
        except (psutil.NoSuchProcess, psutil.AccessDenied):
            break
        stop_event.wait(MEM_POLL_SECS)


# ── Scanner execution ─────────────────────────────────────────────────────────

# Map sensor names to existing scripts
SENSOR_SCRIPTS = {
    "thermal":      "hard_pixel_audit.py",
    "optical":      "lake_michigan_scan.py",
    "triple_lock":  "triple_lock_fusion.py",
    "swot":         "swot_ssh_extractor.py",
    "sar":          "lake_michigan_scan.py",   # SAR path via --mode sar
    "nir_swir":     "wreck_pixel_probe.py",
}

def _run_sensor(bbox: list, sensor: str, output_dir: Path,
                params: dict, interrupt_check) -> dict:
    """
    Run a single sensor scan. Calls interrupt_check() every 60s.
    Returns {"status": "done"|"interrupted"|"failed", "outputs": [...]}
    """
    script = SENSOR_SCRIPTS.get(sensor)
    if not script:
        return {"status": "failed", "error": f"No script for sensor '{sensor}'"}

    script_path = REPO / script
    if not script_path.exists():
        return {"status": "failed", "error": f"Script not found: {script}"}

    output_dir.mkdir(parents=True, exist_ok=True)
    lat_min, lon_min, lat_max, lon_max = bbox
    bbox_str = f"{lat_min},{lon_min},{lat_max},{lon_max}"

    cmd = [sys.executable, str(script_path),
           "--area", bbox_str,
           "--output", str(output_dir)]

    # Forward any extra params as CLI args
    if sensor == "thermal" and "zscore" in params:
        cmd += ["--zscore", str(params["zscore"])]
    if sensor == "sar":
        cmd += ["--mode", "sar"]

    log.info(f"  Running {sensor}: {' '.join(cmd)}")
    log.info(f"  Free RAM before launch: {_free_ram_gb():.1f}GB")
    proc = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT,
                            text=True, encoding="utf-8", errors="replace")

    # Start OOM watchdog for this subprocess
    _wd_stop = threading.Event()
    _wd = threading.Thread(
        target=_watchdog_thread,
        args=(proc, MAX_JOB_RAM_GB, _wd_stop),
        daemon=True, name=f"oom-wd-{proc.pid}"
    )
    _wd.start()

    collected = []
    try:
        while True:
            # Check interrupt every second
            try:
                line = proc.stdout.readline()
            except Exception:
                break
            if line:
                collected.append(line.rstrip())
                if len(collected) % 20 == 0:
                    log.info(f"    [{sensor}] {line.rstrip()}")

            if proc.poll() is not None:
                # Read remaining output
                rest = proc.stdout.read()
                if rest:
                    collected.extend(rest.splitlines())
                break

            if interrupt_check():
                log.warning(f"  Interrupt detected — terminating {sensor} scan")
                proc.terminate()
                try:
                    proc.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    proc.kill()
                return {"status": "interrupted", "outputs": list(output_dir.glob("*"))}

    except Exception as e:
        proc.kill()
        return {"status": "failed", "error": str(e)}

    rc = proc.returncode
    outputs = list(output_dir.glob("**/*"))
    output_files = [str(f) for f in outputs if f.is_file()]

    if rc == 0:
        return {"status": "done", "outputs": output_files, "log": collected[-20:]}
    else:
        tail = "\n".join(collected[-15:])
        return {"status": "failed", "error": f"exit {rc}\n{tail}", "outputs": output_files}


def _execute_job(job: dict) -> tuple:
    """
    Run all sensors for a job. Returns (result_path_or_None, final_status).
    """
    bbox    = json.loads(job["bbox"])
    sensors = json.loads(job["sensors"])
    params  = json.loads(job.get("params") or "{}")
    label   = job.get("label", job["id"])

    safe_label = label.replace(":", "_").replace("/", "_").replace(" ", "_")[:50]
    ts = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%S")
    output_dir = REPO / "outputs" / "scans" / f"{ts}_{safe_label}"

    log.info(f"Job {job['id']} [{label}] bbox={bbox} sensors={sensors}")
    _write_state({
        "worker_id": WORKER_ID, "job_id": job["id"], "label": label,
        "started": datetime.now(timezone.utc).isoformat(),
        "bbox": bbox, "sensors": sensors,
    })

    results = {}
    interrupted = False

    for sensor in sensors:
        if _STOP:
            interrupted = True
            break

        sensor_dir = output_dir / sensor
        res = _run_sensor(
            bbox, sensor, sensor_dir, params,
            interrupt_check=lambda: Q.has_urgent(job["priority"]) or _STOP
        )
        results[sensor] = res

        if res["status"] == "interrupted":
            interrupted = True
            break

        _record_band_score(bbox, sensor, res, label)

    # Write summary
    summary = {
        "job_id": job["id"], "label": label, "bbox": bbox,
        "sensors": results,
        "completed_at": datetime.now(timezone.utc).isoformat(),
        "interrupted": interrupted,
    }
    output_dir.mkdir(parents=True, exist_ok=True)
    summary_path = output_dir / "scan_summary.json"
    summary_path.write_text(json.dumps(summary, indent=2))

    return str(output_dir), "interrupted" if interrupted else "done"


# ── Band score learning ────────────────────────────────────────────────────────

BAND_SCORES_DB = REPO / "db" / "band_scores.json"

def _record_band_score(bbox: list, sensor: str, result: dict, label: str):
    """Record per-tile band performance for the learning layer."""
    try:
        tile_key = f"{round(bbox[0],1)},{round(bbox[1],1)}"  # 0.1° grid key
        scores = {}
        if BAND_SCORES_DB.exists():
            scores = json.loads(BAND_SCORES_DB.read_text())

        if tile_key not in scores:
            scores[tile_key] = {}
        entry = scores[tile_key].get(sensor, {"probes": 0, "hits": 0, "last": None})

        hit = result.get("status") == "done" and bool(result.get("outputs"))
        entry["probes"] += 1
        if hit:
            entry["hits"] += 1
        entry["last"] = datetime.now(timezone.utc).isoformat()
        entry["last_label"] = label

        scores[tile_key][sensor] = entry
        BAND_SCORES_DB.parent.mkdir(parents=True, exist_ok=True)
        BAND_SCORES_DB.write_text(json.dumps(scores, indent=2))
    except Exception as e:
        log.debug(f"Band score write failed: {e}")


# ── IONOS SFTP upload ──────────────────────────────────────────────────────────

def _upload_to_ionos(local_dir: str, job_id: str) -> bool:
    """
    Upload scan output directory to IONOS via SFTP (paramiko if available,
    else falls back to scp subprocess).
    Returns True on success.
    """
    if not IONOS_HOST or not IONOS_USER:
        log.debug("IONOS SFTP not configured — skipping upload")
        return False

    remote_path = f"{IONOS_PATH.rstrip('/')}/{job_id}"
    log.info(f"Uploading {local_dir} → {IONOS_HOST}:{remote_path}")

    try:
        import paramiko
        transport = paramiko.Transport((IONOS_HOST, 22))
        if IONOS_PASS:
            transport.connect(username=IONOS_USER, password=IONOS_PASS)
        else:
            key_path = Path.home() / ".ssh" / "id_ed25519"
            key = paramiko.Ed25519Key.from_private_key_file(str(key_path))
            transport.connect(username=IONOS_USER, pkey=key)

        sftp = paramiko.SFTPClient.from_transport(transport)

        def _mkdir_p(sftp_client, remote):
            parts = remote.strip("/").split("/")
            cur = ""
            for p in parts:
                cur = f"{cur}/{p}"
                try:
                    sftp_client.stat(cur)
                except FileNotFoundError:
                    sftp_client.mkdir(cur)

        _mkdir_p(sftp, remote_path)

        local = Path(local_dir)
        for f in local.rglob("*"):
            if f.is_file():
                rel = f.relative_to(local)
                remote_file = f"{remote_path}/{rel.as_posix()}"
                remote_parent = str(Path(remote_file).parent)
                _mkdir_p(sftp, remote_parent)
                sftp.put(str(f), remote_file)
                log.debug(f"  ↑ {rel}")

        sftp.close()
        transport.close()
        log.info(f"Upload complete → {IONOS_HOST}:{remote_path}")
        return True

    except ImportError:
        # Fallback: rsync/scp
        auth = f"{IONOS_USER}@{IONOS_HOST}"
        cmd = ["scp", "-r", "-o", "StrictHostKeyChecking=no",
               local_dir, f"{auth}:{remote_path}"]
        r = subprocess.run(cmd, capture_output=True, timeout=300)
        if r.returncode == 0:
            log.info("Upload via scp: done")
            return True
        log.warning(f"scp upload failed: {r.stderr.decode()[:200]}")
        return False

    except Exception as e:
        log.warning(f"IONOS upload error: {e}")
        return False


# ── Main worker loop ───────────────────────────────────────────────────────────

def _run_once() -> bool:
    """
    Pick one job, run it. Returns True if a job was found and run.
    """
    global _CURRENT_JOB

    job = Q.pop_next(WORKER_ID)

    if not job:
        # No queued jobs — generate next idle tile
        tile = Q.next_idle_tile()
        if tile is None:
            return False
        jid = Q.push(
            label=tile["label"],
            bbox=tile["bbox"],
            sensors=tile["sensors"],
            priority=Q.PRIORITY_IDLE,
        )
        log.info(f"Idle: queued tile {tile['label']} as job {jid}")
        job = Q.pop_next(WORKER_ID)
        if not job:
            return False

    _CURRENT_JOB = job
    label_str = job.get('label', job['id'])
    log.info(f"Starting job {job['id']} priority={job['priority']} label={label_str}")

    # Pre-flight RAM check — defer low-priority jobs when memory is tight
    if not _check_ram_ok(label_str):
        if job["priority"] >= Q.PRIORITY_IDLE:
            # Idle job: put it back in QUEUED state and back off
            Q.re_queue(job["id"])
            _CURRENT_JOB = None
            time.sleep(POLL_SECS * 6)  # 3-min backoff before retrying idle tiles
            return False
        # User/directed jobs: log warning but proceed anyway
        log.warning(f"Low RAM but job priority={job['priority']} — proceeding anyway")

    try:
        result_dir, final_status = _execute_job(job)
    except Exception as e:
        log.exception(f"Job {job['id']} crashed: {e}")
        Q.mark_failed(job["id"], str(e))
        _CURRENT_JOB = None
        return True

    if final_status == "interrupted":
        # Re-queue so it will be picked up after the urgent job
        Q.re_queue(job["id"])
        log.info(f"Job {job['id']} suspended (preempted by higher priority)")
    elif final_status == "done":
        Q.mark_done(job["id"], result_dir)
        # Try IONOS upload
        if result_dir and _upload_to_ionos(result_dir, job["id"]):
            Q.mark_uploaded(job["id"])
            # Free local disk after successful upload
            if job["priority"] == Q.PRIORITY_IDLE:
                try:
                    shutil.rmtree(result_dir)
                    log.info(f"Freed local disk: removed {result_dir}")
                except Exception:
                    pass
    else:
        Q.mark_failed(job["id"], final_status)

    _CURRENT_JOB = None
    _write_state({"worker_id": WORKER_ID, "idle": True,
                  "last_job": job["id"],
                  "updated": datetime.now(timezone.utc).isoformat()})
    return True


def run_daemon():
    log.info(f"Scan worker starting — id={WORKER_ID} poll={POLL_SECS}s")
    Q.init_db()

    while not _STOP:
        if _PAUSE:
            log.debug("Paused — waiting")
            time.sleep(POLL_SECS)
            continue

        try:
            ran = _run_once()
        except Exception as e:
            log.exception(f"Worker loop error: {e}")
            ran = False

        if not ran:
            # Truly nothing to do — sleep before next idle tile check
            free = _free_ram_gb()
            if free < MIN_FREE_RAM_GB:
                log.info(f"Queue empty + low RAM ({free:.1f}GB) — sleeping longer")
                time.sleep(POLL_SECS * 4)
            else:
                log.debug(f"Queue empty — sleeping {POLL_SECS}s")
                time.sleep(POLL_SECS)

    log.info("Scan worker stopped cleanly")


# ── CLI ───────────────────────────────────────────────────────────────────────

def main():
    ap = argparse.ArgumentParser(description="CESAROPS scan worker daemon")
    ap.add_argument("--once",   action="store_true", help="Run one job then exit")
    ap.add_argument("--status", action="store_true", help="Show current worker state")
    ap.add_argument("--pause",  action="store_true", help="Write pause flag")
    ap.add_argument("--resume", action="store_true", help="Remove pause flag")
    args = ap.parse_args()

    if args.status:
        s = _read_state()
        d = Q.queue_depth()
        print(f"Worker state: {json.dumps(s, indent=2)}")
        print(f"Queue depth:  {d}")
        return

    if args.pause:
        _write_state({**_read_state(), "paused": True})
        print("Worker paused (takes effect at next job boundary)")
        return

    if args.resume:
        st = _read_state()
        st.pop("paused", None)
        _write_state(st)
        print("Worker resumed")
        return

    if args.once:
        Q.init_db()
        ran = _run_once()
        print("Done" if ran else "No jobs")
        return

    run_daemon()


if __name__ == "__main__":
    main()
