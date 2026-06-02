#!/usr/bin/env python3
"""
Summarize completed RS deep-scan rows via RTX thinker when Forge is idle.

Writes:
  var/fleet-catalog/rs_llm_summaries.jsonl  — one line per path
  var/fleet-catalog/rs_llm_summarize_progress.json — monitor state
"""
from __future__ import annotations

import argparse
import json
import os
import time
from pathlib import Path
from typing import Any

import requests

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
CATALOG_DIR = Path(os.environ.get("FLEET_CATALOG_DIR", REPO / "var/fleet-catalog"))
FORGE_URL = os.environ.get("FORGE_URL", "http://127.0.0.1:9100").rstrip("/")
THINKER_URL = os.environ.get("THINKER_URL", "http://127.0.0.1:5200").rstrip("/")
MASTER = Path(os.environ.get("RS_SUMMARIZE_MASTER", CATALOG_DIR / "fleet_master_deep.jsonl"))
if not MASTER.is_file():
    MASTER = CATALOG_DIR / "fleet_master.jsonl"
OUT_SUMMARIES = CATALOG_DIR / "rs_llm_summaries.jsonl"
PROGRESS = CATALOG_DIR / "rs_llm_summarize_progress.json"
LOG_PATH = Path(os.environ.get("RS_SUMMARIZE_LOG", CATALOG_DIR / "rs_llm_summarize.log"))

RS_EXTS = {
    ".tif", ".tiff", ".nc", ".nc4", ".hdf", ".h5", ".bag", ".shp", ".geojson",
    ".kml", ".las", ".laz", ".jp2", ".vrt", ".tfw", ".prj",
}
RS_DENY_SUFFIX = {
    ".py", ".rs", ".toml", ".lock", ".md", ".json", ".txt", ".csv", ".sh", ".yml",
    ".yaml", ".exe", ".log",
}


def log(msg: str) -> None:
    line = f"[rs-llm] {msg}"
    print(line, flush=True)
    LOG_PATH.parent.mkdir(parents=True, exist_ok=True)
    with LOG_PATH.open("a", encoding="utf-8") as f:
        f.write(f"{time.strftime('%Y-%m-%d %H:%M:%S')} {line}\n")


def load_progress() -> dict[str, Any]:
    if PROGRESS.is_file():
        return json.loads(PROGRESS.read_text(encoding="utf-8"))
    return {}


def save_progress(prog: dict[str, Any]) -> None:
    PROGRESS.parent.mkdir(parents=True, exist_ok=True)
    PROGRESS.write_text(json.dumps(prog, indent=2), encoding="utf-8")


def forge_idle() -> tuple[bool, str]:
    try:
        status = requests.get(f"{FORGE_URL}/forge/status", timeout=5).json()
    except requests.RequestException as e:
        return False, f"forge_unreachable:{e}"
    if status.get("send_busy"):
        return False, "send_busy"
    try:
        missions = requests.get(f"{FORGE_URL}/webhook/missions", timeout=5).json()
        arr = missions if isinstance(missions, list) else missions.get("missions", [])
        for m in arr:
            if isinstance(m, dict) and str(m.get("status", "")).lower() == "running":
                return False, "mission_running"
    except requests.RequestException:
        pass
    return True, "idle"


def thinker_ok() -> bool:
    try:
        r = requests.get(f"{THINKER_URL}/v1/models", timeout=5)
        return r.ok
    except requests.RequestException:
        return False


def thinker_model_id() -> str:
    env = os.environ.get("THINKER_MODEL", "").strip()
    if env:
        return env
    try:
        r = requests.get(f"{THINKER_URL}/v1/models", timeout=8)
        r.raise_for_status()
        data = r.json().get("data") or r.json().get("models") or []
        if data:
            return data[0].get("id") or data[0].get("model") or data[0].get("name")
    except requests.RequestException:
        pass
    return "default"


def is_real_rs_row(row: dict[str, Any]) -> bool:
    path = row.get("path", "")
    if not path or path.startswith("sqlite:"):
        return False
    ext = Path(path).suffix.lower()
    if ext in RS_DENY_SUFFIX:
        return False
    if ext in RS_EXTS or path.endswith(".aux.xml"):
        return row.get("deep_scan_status") == "complete" or bool(row.get("examine_blob"))
    prof = row.get("rs_profile") or {}
    if prof.get("gdalinfo") or prof.get("global_attrs") or prof.get("netcdf_dims"):
        return row.get("deep_scan_status") == "complete"
    return False


def load_summarized_paths() -> set[str]:
    done: set[str] = set()
    if not OUT_SUMMARIES.is_file():
        return done
    with OUT_SUMMARIES.open(encoding="utf-8") as f:
        for line in f:
            if line.strip():
                done.add(json.loads(line).get("path", ""))
    return done


def load_master_rows() -> list[dict[str, Any]]:
    if not MASTER.is_file():
        return []
    return [json.loads(line) for line in MASTER.open(encoding="utf-8") if line.strip()]


def build_prompt(row: dict[str, Any]) -> str:
    prof = row.get("rs_profile") or {}
    # Thinker :5200 uses -c 3072; keep prompt well under context (Cargo.lock blobs were 400'ing).
    blob = (row.get("examine_blob") or "")[:3500]
    return f"""You summarize ONE remote-sensing asset for CESAROPS consolidation planning.

PATH: {row.get('path')}
SHA256: {row.get('sha256', '')[:16]}…
SENSOR_GUESS: {row.get('sensor_guess', prof.get('sensor_guess', 'unknown'))}
SIZE_BYTES: {row.get('size_bytes')}

RS_PROFILE (JSON):
{json.dumps(prof, default=str)[:2000]}

EXAMINE_BLOB:
{blob}

Reply with exactly these sections (plain text, under 250 words total):
## target_repo
(one of: wreckhunter2000-1 | cesarops-detection | sonarsniffer | universal_downloader | data-archive | review)
## recommended_action
(catalog_data | move_to_repo | archive | quarantine | review_rs)
## consolidation_note
(2-4 sentences: what this file is, lake/mission relevance, duplicate risk)
## risks
(bullet list, max 3)
"""


def call_thinker(prompt: str) -> str:
    model = thinker_model_id()
    body = {
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": (
                    "You are the Forge thinker summarizing remote-sensing catalog entries. "
                    "Be specific to CESAROPS Great Lakes wreck hunting. No code. Use the required sections."
                ),
            },
            {"role": "user", "content": prompt},
        ],
        "temperature": 0.2,
        "max_tokens": int(os.environ.get("RS_SUMMARIZE_MAX_TOKENS", "768")),
    }
    r = requests.post(f"{THINKER_URL}/v1/chat/completions", json=body, timeout=300)
    if not r.ok:
        raise RuntimeError(f"thinker {r.status_code}: {r.text[:400]}")
    msg = r.json()["choices"][0]["message"]
    text = (msg.get("content") or "").strip()
    if not text and msg.get("reasoning_content"):
        text = (msg.get("reasoning_content") or "").strip()
    if not text:
        raise RuntimeError("thinker returned empty content and reasoning")
    return text.strip()


def parse_sections(text: str) -> dict[str, str]:
    out: dict[str, str] = {}
    current = ""
    for line in text.splitlines():
        if line.startswith("## "):
            current = line[3:].strip().lower().replace(" ", "_")
            out[current] = ""
        elif current:
            out[current] += line + "\n"
    for k in list(out.keys()):
        out[k] = out[k].strip()
    return out


def summarize_one(row: dict[str, Any]) -> dict[str, Any]:
    raw = call_thinker(build_prompt(row))
    sections = parse_sections(raw)
    return {
        "path": row.get("path"),
        "sha256": row.get("sha256"),
        "summarized_at": int(time.time()),
        "thinker_url": THINKER_URL,
        "raw_summary": raw,
        "target_repo": sections.get("target_repo", "review"),
        "recommended_action": sections.get("recommended_action", "review_rs"),
        "consolidation_note": sections.get("consolidation_note", ""),
        "risks": sections.get("risks", ""),
    }


def patch_master_with_summaries() -> int:
    if not OUT_SUMMARIES.is_file() or not MASTER.is_file():
        return 0
    by_path = {}
    with OUT_SUMMARIES.open(encoding="utf-8") as f:
        for line in f:
            if line.strip():
                s = json.loads(line)
                by_path[s["path"]] = s
    lines: list[str] = []
    n = 0
    for line in MASTER.open(encoding="utf-8"):
        if not line.strip():
            continue
        row = json.loads(line)
        p = row.get("path")
        if p in by_path:
            row["llm_summary"] = by_path[p]
            n += 1
        lines.append(json.dumps(row, ensure_ascii=False))
    patched = CATALOG_DIR / "fleet_master_summarized.jsonl"
    patched.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return n


def run_batch(rows: list[dict[str, Any]], prog: dict[str, Any]) -> None:
    OUT_SUMMARIES.parent.mkdir(parents=True, exist_ok=True)
    with OUT_SUMMARIES.open("a", encoding="utf-8") as out:
        for row in rows:
            path = row.get("path", "")
            try:
                rec = summarize_one(row)
                out.write(json.dumps(rec, ensure_ascii=False) + "\n")
                out.flush()
                prog["done"] = int(prog.get("done", 0)) + 1
                prog["last_path"] = path
                prog["last_ok_at"] = int(time.time())
                log(f"ok {prog['done']}/{prog.get('total', '?')} {path[-70:]}")
            except Exception as e:
                prog["errors"] = int(prog.get("errors", 0)) + 1
                prog["last_error"] = str(e)[:300]
                log(f"err {path[-60:]}: {e}")
            save_progress(prog)


def pending_rows(max_total: int) -> tuple[list[dict[str, Any]], set[str]]:
    summarized = load_summarized_paths()
    pending = [
        r for r in load_master_rows()
        if is_real_rs_row(r) and r.get("path") not in summarized
    ]
    if max_total:
        pending = pending[: max(0, max_total - len(summarized))]
    return pending, summarized


def run_watch(poll_sec: int, batch_size: int, max_total: int) -> None:
    prog = load_progress()
    prog.setdefault("started_at", int(time.time()))
    prog["mode"] = "watch"
    prog["poll_sec"] = poll_sec
    save_progress(prog)

    pending, summarized = pending_rows(max_total)
    log(f"queue {len(pending)} RS files (already done {len(summarized)})")

    while True:
        pending, summarized = pending_rows(max_total)
        prog["done"] = len(summarized)
        prog["queue_pending"] = len(pending)
        prog["total"] = len(summarized) + len(pending)
        if not pending:
            prog["status"] = (
                "idle_no_eligible_rs"
                if prog.get("queue_pending", 0) == 0 and prog.get("done", 0) >= 0
                else "waiting_for_rs_deep"
            )
            save_progress(prog)
            time.sleep(poll_sec)
            continue

        idle, reason = forge_idle()
        prog["last_forge_state"] = reason
        if not idle:
            prog["forge_busy_waits"] = int(prog.get("forge_busy_waits", 0)) + 1
            save_progress(prog)
            if prog["forge_busy_waits"] % 5 == 1:
                log(f"forge busy ({reason}) — waiting {poll_sec}s")
            time.sleep(poll_sec)
            continue
        if not thinker_ok():
            log(f"thinker down at {THINKER_URL} — waiting {poll_sec}s")
            time.sleep(poll_sec)
            continue

        batch = pending[:batch_size]
        prog["status"] = "summarizing"
        save_progress(prog)
        log(f"forge idle — summarizing batch of {len(batch)}")
        run_batch(batch, prog)
        prog["master_patched_rows"] = patch_master_with_summaries()
        prog["status"] = "idle_between_batches"
        save_progress(prog)
        time.sleep(2)


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--watch", action="store_true", help="Wait for Forge idle between batches")
    ap.add_argument("--poll-sec", type=int, default=30)
    ap.add_argument("--batch-size", type=int, default=3)
    ap.add_argument("--max-total", type=int, default=0)
    ap.add_argument("--once", action="store_true", help="Single batch if idle, else exit")
    ap.add_argument("--status", action="store_true")
    ap.add_argument("--patch-master", action="store_true")
    args = ap.parse_args()

    if args.status:
        prog = load_progress()
        idle, reason = forge_idle()
        print(json.dumps({"forge_idle": idle, "forge_reason": reason, "thinker_ok": thinker_ok(), **prog}, indent=2))
        return 0

    if args.patch_master:
        n = patch_master_with_summaries()
        log(f"patched {n} rows -> {CATALOG_DIR / 'fleet_master_summarized.jsonl'}")
        return 0

    if args.watch:
        run_watch(args.poll_sec, args.batch_size, args.max_total)
        return 0

    idle, reason = forge_idle()
    if not idle:
        log(f"forge not idle ({reason}) — use --watch or retry later")
        return 2
    if not thinker_ok():
        log(f"thinker not ready at {THINKER_URL}")
        return 2

    summarized = load_summarized_paths()
    pending = [
        r for r in load_master_rows()
        if is_real_rs_row(r) and r.get("path") not in summarized
    ]
    if args.max_total:
        pending = pending[: args.max_total]
    if not pending:
        log("nothing to summarize")
        return 0
    prog = load_progress()
    prog["total"] = len(summarized) + len(pending)
    prog["done"] = len(summarized)
    run_batch(pending[: args.batch_size], prog)
    patch_master_with_summaries()
    save_progress(prog)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
