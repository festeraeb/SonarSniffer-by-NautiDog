#!/usr/bin/env python3
"""Dual-P100 Rust port for laptop-dump manifest only (excludes cesarops2 / 2060 queue)."""
from __future__ import annotations

import json
import os
import time
import textwrap
import threading
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/codebase/repos/wreckhunter2000-1"))
MANIFEST = REPO / "integrate" / "_MANIFEST.json"
C2060_QUEUE = REPO / "integrate" / "PYTHON_TO_RUST_QUEUE.json"
OUT = Path(os.environ.get("OUT", REPO / "integrate_out" / "split_rust"))
QUEUE_OUT = REPO / "integrate" / "MANIFEST_RUST_QUEUE.json"
LOG = Path(os.environ.get("LOG", "/tmp/manifest_rust_dispatch.log"))

P1000_URL = os.environ.get("P1000_URL", "http://127.0.0.1:5001/v1/chat/completions")
P1001_URL = os.environ.get("P1001_URL", "http://127.0.0.1:5002/v1/chat/completions")
MAX_SOURCE = int(os.environ.get("MAX_SOURCE", "12000"))
MAX_TOKENS = int(os.environ.get("MAX_TOKENS", "8192"))
TIMEOUT = int(os.environ.get("TIMEOUT", "300"))
WORKERS = int(os.environ.get("WORKERS", "2"))
HEAD_N = int(os.environ.get("HEAD_N", "0"))  # 0 = all
ONLY = os.environ.get("ONLY", "").strip()  # comma-separated basenames without .py

# Already in cesarops-inference/src/integrate/ — skip
AGENT_RUST = {
    "cesarops_engine.py",
    "analyze_fuel_leaks.py",
    "lake_michigan_full_scan.py",
    "multi_sensor_scan.py",
    "prioritized_pull_v2.py",
    "daily_satellite_pull.py",
    "cuda_direct.py",
    "integrated_forensic_scan.py",
    "cesarops_cli.py",
    "cesarops_agent_gui.py",
    "full_basin_scan.py",
    "triple_lock_fusion.py",
    "ai_director.py",
    "analyze_crossref.py",
    "drive_identity.py",
    "dynamic_db_key.py",
    "dual_scan_downloader.py",
    "detection_sorter.py",
    "fast_scan.py",
    "analyze_resolution_comparison.py",
    "anchor_lock_display.py",
    "check_xenon_cuda.py",
    "daily_scan.py",
    "diagnose_gpu.py",
    "lake_michigan_scan.py",
    "hard_pixel_audit.py",
    "monster_material_audit.py",
    "inspect_scan.py",
    "download_straits_2024.py",
    "bridge_proximity.py",
    "mb2_crosscheck.py",
    "wipe_database.py",
    "query_wrecks_db.py",
    "scan_wreck_db.py",
    "match_wrecks.py",
    "hls_download.py",
    "hls_dl.py",
    "hls_dl2.py",
    "hls_download2.py",
    "hls_download3.py",
    "cleanup_and_organize.py",
    "inspect_geometry_metadata.py",
    "wreck_pixel_probe.py",
    "generate_report.py",
    "crossref_scans.py",
    "fix_and_add_wrecks.py",
    "fetch_geometry_metadata.py",
    "tile_geometry.py",
    "tile_selector.py",
    "deep_wreck_validation.py",
    "llm_context_injector.py",
    "deploy_and_scan.py",
    "remote_dispatch.py",
    "cesarops_orchestrator.py",
}

SYSTEM = """You are a senior CESAROPS Rust engineer on the T440 fleet.
Convert the Python laptop-dump script into production Rust for cesarops-inference.

Output markdown ONLY:
## Verdict
PORT_TO_PIPELINES
## Rust path
exact path given in the user message
## Rust source
Full ```rust ... ``` module (no placeholders, no todo!, compilable)
## Forge wire
How Forge/pipeline calls this (1-3 bullets)
## Risks
Brief bullets
Start with ## Verdict. No chain-of-thought."""

_log_lock = threading.Lock()


def log(msg: str) -> None:
    line = f"[manifest-rust] {msg}"
    with _log_lock:
        print(line, flush=True)
        with LOG.open("a", encoding="utf-8") as f:
            f.write(line + "\n")


def base_url(chat_url: str) -> str:
    chat_url = chat_url.rstrip("/")
    return chat_url[: -len("/v1/chat/completions")] if chat_url.endswith("/v1/chat/completions") else chat_url


def wait_ready(chat_url: str, timeout_s: int = 90) -> bool:
    """Wait until /health or /v1/models responds OK (handles model-loading 503)."""
    start = time.time()
    api = base_url(chat_url)
    while time.time() - start < timeout_s:
        for path in ("/health", "/v1/models"):
            try:
                with urllib.request.urlopen(f"{api}{path}", timeout=5) as resp:
                    status = getattr(resp, "status", 200)
                    if 200 <= status < 300:
                        return True
            except urllib.error.HTTPError as e:
                # model loading often returns 503 for a bit
                if e.code in (503, 502, 500):
                    pass
            except OSError:
                pass
        time.sleep(1.5)
    return False


def chat(url: str, user: str, max_tokens: int, system: str = SYSTEM) -> str:
    # Avoid spamming a server while it's still loading a model.
    wait_ready(url, timeout_s=90)
    body = {
        "model": "qwen",
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
        "max_tokens": max_tokens,
        "temperature": 0.15,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    # llama-server can briefly refuse connections under load; retry with backoff.
    last: Exception | None = None
    for i in range(6):
        try:
            with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
                data = json.loads(resp.read().decode())
            return data["choices"][0]["message"].get("content") or ""
        except urllib.error.URLError as e:
            last = e
            # Connection refused / temporary disconnect
            time.sleep(1.0 + i * 1.5)
            wait_ready(url, timeout_s=30)
            continue
        except OSError as e:
            last = e
            time.sleep(1.0 + i * 1.5)
            wait_ready(url, timeout_s=30)
            continue
    assert last is not None
    raise last


def plan_ok(path: Path) -> bool:
    if not path.is_file():
        return False
    text = path.read_text(encoding="utf-8", errors="replace")
    return len(text) > 400 and "## Verdict" in text and "```rust" in text


def best_plan(base: str) -> Path | None:
    for tag in ("p1000", "p1001"):
        p = OUT / tag / f"rust__{base}.md"
        if plan_ok(p):
            return p
    return None


def build_manifest_queue() -> list[dict]:
    c2060 = {e["source_py"] for e in json.loads(C2060_QUEUE.read_text(encoding="utf-8"))}
    by_name: dict[str, dict] = {}
    for row in json.loads(MANIFEST.read_text(encoding="utf-8")):
        dest = row["dest"]
        name = Path(dest).name
        if name in c2060 or name in AGENT_RUST:
            continue
        if name.startswith("test_"):
            continue
        if "SNAPSHOT" in dest or "archived/" in dest:
            continue
        base = name.replace(".py", "")
        rust_path = f"cesarops-inference/src/integrate/{base}.rs"
        entry = {
            "source_py": name,
            "python_path": str(REPO / dest),
            "manifest_dest": dest,
            "rust_path": rust_path,
            "lines": row.get("lines", 0),
            "stub_score": row.get("stub_score", 0),
        }
        prev = by_name.get(name)
        if not prev:
            by_name[name] = entry
            continue
        # Prefer wreckhunter_build, then programming_root
        rank = lambda d: (
            "wreckhunter_build" not in d,
            "programming_root" not in d,
            len(d),
        )
        if rank(dest) < rank(prev["manifest_dest"]):
            by_name[name] = entry
    items = sorted(by_name.values(), key=lambda x: (x["stub_score"], x["lines"], x["source_py"]))
    return items


def pick_worker(entry: dict) -> tuple[str, str]:
    """Small/stub scripts → p1001 reviewer; larger → p1000 MoE."""
    if entry.get("stub_score", 0) == 0 and entry.get("lines", 0) < 200:
        return P1001_URL, "p1001"
    return P1000_URL, "p1000"


def fallback_worker(url: str, tag: str) -> tuple[str, str]:
    if tag == "p1000":
        return P1001_URL, "p1001"
    return P1000_URL, "p1000"


def dispatch_one(entry: dict, url: str, tag: str) -> dict:
    base = entry["source_py"].replace(".py", "")
    out_md = OUT / tag / f"rust__{base}.md"
    py_path = Path(entry["python_path"])
    if not py_path.is_file():
        return {"source": entry["source_py"], "error": f"missing {py_path}"}
    if plan_ok(out_md) or best_plan(base):
        return {"source": entry["source_py"], "out": str(out_md), "skipped": True}

    last_err = ""
    # Profiles tuned for P100 llama context/KV limits:
    # 1) normal pass, 2) corrector pass on other worker, 3) translator/minimal-context pass.
    profiles: list[dict] = [
        {
            "role": "thinker",
            "source_chars": MAX_SOURCE,
            "max_tokens": MAX_TOKENS,
            "url": url,
            "tag": tag,
            "system": SYSTEM,
            "hint": "Produce full module + tests in one pass.",
        },
        {
            "role": "corrector",
            "source_chars": min(MAX_SOURCE, 9000),
            "max_tokens": min(MAX_TOKENS, 3072),
            "url": fallback_worker(url, tag)[0],
            "tag": fallback_worker(url, tag)[1],
            "system": SYSTEM
            + "\n"
            + "You are the CORRECTOR pass. Prioritize validity/compilability and concise implementation over breadth.",
            "hint": "If context is tight, keep only the highest-value functions and tests.",
        },
        {
            "role": "translator",
            "source_chars": min(MAX_SOURCE, 7000),
            "max_tokens": min(MAX_TOKENS, 2048),
            "url": fallback_worker(url, tag)[0],
            "tag": fallback_worker(url, tag)[1],
            "system": SYSTEM
            + "\n"
            + "You are the TRANSLATOR pass. Minimal robust API, deterministic behavior, and at least 2 meaningful tests.",
            "hint": "Focus on core behavior, serde types, and testable helpers.",
        },
    ]
    for attempt, profile in enumerate(profiles, start=1):
        try:
            body = py_path.read_text(encoding="utf-8", errors="replace")[: profile["source_chars"]]
            user = textwrap.dedent(
                f"""
                Convert this laptop-dump Python script to Rust for cesarops-inference.

                Manifest path: {entry['manifest_dest']}
                Target Rust path: {entry['rust_path']}
                lines: {entry.get('lines')} stub_score: {entry.get('stub_score')}

                Match existing integrate modules (pub fn API, unit tests, std/serde/ndarray only).
                {profile['hint']}

                --- source ---
                {body}
                """
            ).strip()
            log(
                f"{profile['tag']} {entry['source_py']} attempt {attempt}/3 "
                f"role={profile['role']} src={profile['source_chars']} tok={profile['max_tokens']}"
            )
            reply = chat(
                profile["url"],
                user,
                max_tokens=profile["max_tokens"],
                system=profile["system"],
            )
            if len(reply.strip()) < 120 or "## Verdict" not in reply or "```rust" not in reply:
                last_err = f"invalid reply ({len(reply)} bytes)"
                continue
            out_md.parent.mkdir(parents=True, exist_ok=True)
            out_md.write_text(f"# {entry['manifest_dest']}\n\n{reply}\n", encoding="utf-8")
            return {"source": entry["source_py"], "out": str(out_md), "tag": profile["tag"]}
        except urllib.error.HTTPError as e:
            detail = ""
            try:
                detail = e.read().decode(errors="replace")[:300]
            except Exception:
                pass
            last_err = f"{e} {detail}".strip()
        except (TimeoutError, OSError, ValueError, json.JSONDecodeError) as e:
            last_err = str(e)
    return {"source": entry["source_py"], "error": last_err, "tag": tag}


def main() -> None:
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "p1000").mkdir(exist_ok=True)
    (OUT / "p1001").mkdir(exist_ok=True)

    queue = build_manifest_queue()
    QUEUE_OUT.write_text(json.dumps(queue, indent=2) + "\n", encoding="utf-8")

    todo = []
    for entry in queue:
        base = entry["source_py"].replace(".py", "")
        if best_plan(base):
            log(f"SKIP done {entry['source_py']}")
            continue
        if ONLY:
            allow = {x.strip() for x in ONLY.split(",") if x.strip()}
            if base not in allow:
                continue
        todo.append(entry)
    if HEAD_N > 0:
        todo = todo[:HEAD_N]

    log(f"manifest queue={len(queue)} todo={len(todo)} (excludes c2060 51 + agent ports)")
    log(f"workers={WORKERS} timeout={TIMEOUT}s log={LOG}")

    results = []
    with ThreadPoolExecutor(max_workers=WORKERS) as pool:
        futures = []
        for entry in todo:
            url, tag = pick_worker(entry)
            futures.append(pool.submit(dispatch_one, entry, url, tag))
        for fut in as_completed(futures):
            row = fut.result()
            results.append(row)
            if row.get("error"):
                log(f"ERR {row['source']}: {row['error']}")
            elif not row.get("skipped"):
                log(f"OK {row['source']} -> {row.get('out')}")

    summary = OUT / "manifest_rust_summary.json"
    summary.write_text(json.dumps(results, indent=2) + "\n", encoding="utf-8")
    ok = sum(1 for r in results if r.get("out") and not r.get("skipped"))
    err = sum(1 for r in results if r.get("error"))
    log(f"finished ok={ok} err={err} skipped={len(queue)-len(todo)}")


if __name__ == "__main__":
    main()
