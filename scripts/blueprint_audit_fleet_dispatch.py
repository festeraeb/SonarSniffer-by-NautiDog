#!/usr/bin/env python3
"""
Dispatch blueprint audit LLM batches across the inference fleet.

Tries direct worker /v1/chat/completions first (P100 Gemma/R1, cesarops2).
Optional Forge /send per batch; on failure records reason and continues on workers.
Writes deep results to reports/blueprint_audit/llm_results/deep/ and a run manifest.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import time
import urllib.error
import urllib.request
from concurrent.futures import ThreadPoolExecutor, as_completed
from dataclasses import dataclass
from pathlib import Path
from typing import Any, Dict, List, Optional, Tuple


DEFAULT_WORKERS = [
    {"id": "p100-gemma", "url": "http://127.0.0.1:5001"},
    {"id": "p100-r1", "url": "http://127.0.0.1:5002"},
    {"id": "c2-qwen", "url": "http://10.0.0.201:5200"},
    {"id": "c2-gemma", "url": "http://10.0.0.201:5201"},
]

FORGE_URL = "http://127.0.0.1:9100"
DEFAULT_CHUNK_SIZE = 20
DEFAULT_MIN_CHUNK_SIZE = 6
MAX_TOKENS = 8192
TEMPERATURE = 0.2
TIMEOUT_S = 600


@dataclass
class Worker:
    id: str
    url: str


@dataclass
class WorkerState:
    worker: Worker
    model_id: str
    model_score: int


def load_jsonl(path: Path) -> Dict[str, Dict[str, Any]]:
    out: Dict[str, Dict[str, Any]] = {}
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            out[row["path"]] = row
    return out


def compact_row(rec: Dict[str, Any]) -> str:
    mods = ",".join(rec.get("modules") or [])
    return (
        f"{rec['path']}|{rec.get('primary_module','')}|{rec.get('guessed_role','')}"
        f"|{mods}|{rec.get('module_score',0)}|{rec.get('overlaps',0)}"
        f"|{rec.get('language','')}|{rec.get('lines',0)}"
    )


def build_chunk_prompt(instruction: str, rows: List[Dict[str, Any]]) -> str:
    lines = [compact_row(r) for r in rows]
    table = "\n".join(lines)
    return f"""You are auditing a wreck-hunting geospatial codebase against a 5-module blueprint
(geometry_context, spectral_plume_clarity, sar_ripple_glint, thermal_wake, altimetry_ssh).

{instruction}

Input format per line: path|primary_module|guessed_role|modules_csv|module_score|overlaps|language|lines

Files:
{table}

Return ONLY a JSON array. Each element:
{{"path":"...","purpose_summary":"...","primary_module":"...","modules":["..."],
"overlap_redundancy":"overlap_detected|no_blueprint_overlap","usefulness_1_to_5":1-5,
"guessed_role":"...","notes":"..."}}
No markdown fences."""


def http_post_json(url: str, payload: Dict[str, Any], timeout: int) -> Tuple[int, str]:
    data = json.dumps(payload).encode("utf-8")
    req = urllib.request.Request(
        url,
        data=data,
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace") if e.fp else str(e)
        return e.code, body
    except Exception as e:
        return 0, str(e)


def http_get_text(url: str, timeout: int) -> Tuple[int, str]:
    req = urllib.request.Request(url, method="GET")
    try:
        with urllib.request.urlopen(req, timeout=timeout) as resp:
            return resp.status, resp.read().decode("utf-8", errors="replace")
    except urllib.error.HTTPError as e:
        body = e.read().decode("utf-8", errors="replace") if e.fp else str(e)
        return e.code, body
    except Exception as e:
        return 0, str(e)


def model_score(model_id: str) -> int:
    m = re.search(r"(\d+)\s*B", model_id, re.IGNORECASE)
    if not m:
        return 0
    try:
        return int(m.group(1))
    except ValueError:
        return 0


def fetch_worker_model_id(worker: Worker) -> Tuple[Optional[str], Optional[str]]:
    status, body = http_get_text(worker.url.rstrip("/") + "/v1/models", timeout=8)
    if status != 200:
        return None, f"{worker.id} /v1/models HTTP {status}: {body[:180]}"
    try:
        doc = json.loads(body)
    except json.JSONDecodeError as e:
        return None, f"{worker.id} /v1/models invalid JSON: {e}"

    model_id = ""
    data = doc.get("data")
    if isinstance(data, list) and data:
        first = data[0]
        if isinstance(first, dict):
            model_id = str(first.get("id") or first.get("model") or "")
    if not model_id:
        models = doc.get("models")
        if isinstance(models, list) and models:
            first = models[0]
            if isinstance(first, dict):
                model_id = str(first.get("id") or first.get("name") or first.get("model") or "")
    if not model_id:
        return None, f"{worker.id} /v1/models missing model id"
    return model_id, None


def filter_workers(
    workers: List[Worker],
    allow_re: Optional[str],
    deny_re: Optional[str],
    min_model_b: int,
) -> Tuple[List[WorkerState], List[str]]:
    errors: List[str] = []
    out: List[WorkerState] = []

    allow = re.compile(allow_re, re.IGNORECASE) if allow_re else None
    deny = re.compile(deny_re, re.IGNORECASE) if deny_re else None

    for w in workers:
        model_id, err = fetch_worker_model_id(w)
        if err:
            errors.append(err)
            continue
        mid = model_id or ""
        if allow and not allow.search(mid):
            errors.append(f"{w.id} model filtered by allow regex: {mid}")
            continue
        if deny and deny.search(mid):
            errors.append(f"{w.id} model filtered by deny regex: {mid}")
            continue
        score = model_score(mid)
        if score < min_model_b:
            errors.append(f"{w.id} model below min size {min_model_b}B: {mid}")
            continue
        out.append(WorkerState(worker=w, model_id=mid, model_score=score))

    out.sort(key=lambda ws: ws.model_score, reverse=True)
    return out, errors


def chat_complete(worker: Worker, prompt: str) -> Tuple[Optional[str], Optional[str]]:
    url = worker.url.rstrip("/") + "/v1/chat/completions"
    payload = {
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": MAX_TOKENS,
        "temperature": TEMPERATURE,
        "stream": False,
    }
    status, body = http_post_json(url, payload, TIMEOUT_S)
    if status != 200:
        return None, f"{worker.id} HTTP {status}: {body[:500]}"
    try:
        doc = json.loads(body)
    except json.JSONDecodeError:
        return None, f"{worker.id} invalid JSON body"
    if doc.get("error"):
        return None, f"{worker.id} error: {doc['error']}"
    choices = doc.get("choices") or []
    if not choices:
        return None, f"{worker.id} empty choices"
    msg = choices[0].get("message") or {}
    text = msg.get("content") or choices[0].get("text") or ""
    if not text.strip():
        return None, f"{worker.id} empty completion"
    return text, None


def extract_json_array(text: str) -> Tuple[Optional[List[Any]], Optional[str]]:
    text = text.strip()
    if text.startswith("```"):
        text = re.sub(r"^```[a-zA-Z]*\n?", "", text)
        text = re.sub(r"\n?```\s*$", "", text)
    try:
        val = json.loads(text)
        if isinstance(val, list):
            return val, None
        if isinstance(val, dict) and "results" in val:
            r = val["results"]
            if isinstance(r, list):
                return r, None
    except json.JSONDecodeError:
        pass
    m = re.search(r"\[[\s\S]*\]", text)
    if m:
        try:
            val = json.loads(m.group(0))
            if isinstance(val, list):
                return val, None
        except json.JSONDecodeError as e:
            return None, f"json parse: {e}"
    first = text.find("[")
    last = text.rfind("]")
    if first >= 0 and last > first:
        candidate = text[first : last + 1]
        try:
            val = json.loads(candidate)
            if isinstance(val, list):
                return val, None
        except json.JSONDecodeError as e:
            return None, f"json parse: {e}"
    return None, "could not extract JSON array from model output"


def try_forge_batch(batch_id: str, instruction: str, n_files: int) -> Tuple[bool, str]:
    prompt = (
        f"/fast blueprint audit {batch_id}: {instruction} "
        f"({n_files} files; respond with compact JSON array only, no prose)."
    )
    payload = {"message": prompt, "lane": "reviewer"}
    status, body = http_post_json(f"{FORGE_URL}/send", payload, 120)
    if status != 200:
        return False, f"forge HTTP {status}: {body[:300]}"
    return True, "forge accepted (async; not waiting for completion)"


def worker_ring(workers: List[Worker], start: int) -> List[Worker]:
    n = len(workers)
    return [workers[(start + i) % n] for i in range(n)]


def process_chunk(
    workers: List[Worker],
    worker_idx: int,
    instruction: str,
    rows: List[Dict[str, Any]],
    chunk_label: str,
) -> Tuple[List[Dict[str, Any]], str, List[str]]:
    failures: List[str] = []
    prompt = build_chunk_prompt(instruction, rows)
    for w in worker_ring(workers, worker_idx):
        text, err = chat_complete(w, prompt)
        if err:
            failures.append(err)
            continue
        parsed, perr = extract_json_array(text or "")
        if perr:
            failures.append(f"{w.id} {perr}")
            continue
        return parsed, w.id, failures
    return [], "", failures


def should_split_retry(failures: List[str]) -> bool:
    if not failures:
        return False
    hit_terms = (
        "Context size has been exceeded",
        "timed out",
        "json parse",
        "invalid JSON body",
        "empty completion",
        "could not extract JSON array",
    )
    joined = "\n".join(failures)
    return any(t in joined for t in hit_terms)


def process_rows_adaptive(
    workers: List[Worker],
    worker_idx: int,
    instruction: str,
    rows: List[Dict[str, Any]],
    chunk_label: str,
    min_chunk_size: int,
) -> Tuple[List[Dict[str, Any]], List[str], List[str]]:
    parsed, wid, fails = process_chunk(workers, worker_idx, instruction, rows, chunk_label)
    used = [wid] if wid else []
    if parsed:
        return parsed, used, fails

    if len(rows) <= max(1, min_chunk_size) or not should_split_retry(fails):
        return [], used, fails

    mid = len(rows) // 2
    left = rows[:mid]
    right = rows[mid:]

    lres, lused, lfails = process_rows_adaptive(
        workers,
        worker_idx,
        instruction,
        left,
        f"{chunk_label}.L",
        min_chunk_size,
    )
    rres, rused, rfails = process_rows_adaptive(
        workers,
        (worker_idx + 1) % max(1, len(workers)),
        instruction,
        right,
        f"{chunk_label}.R",
        min_chunk_size,
    )

    merged_used = list(dict.fromkeys(lused + rused))
    merged_fails = fails + lfails + rfails
    return lres + rres, merged_used, merged_fails


def process_batch(
    batch: Dict[str, Any],
    inventory: Dict[str, Dict[str, Any]],
    workers: List[Worker],
    worker_idx: int,
    try_forge: bool,
    out_dir: Path,
    chunk_size: int,
    min_chunk_size: int,
) -> Dict[str, Any]:
    batch_id = str(batch["batch_id"])
    instruction = str(batch.get("instruction", ""))
    paths = list(batch.get("paths", []))
    rows: List[Dict[str, Any]] = []
    missing: List[str] = []
    for p in paths:
        rec = inventory.get(p)
        if rec:
            rows.append(rec)
        else:
            missing.append(p)

    record: Dict[str, Any] = {
        "batch_id": batch_id,
        "status": "pending",
        "worker_primary": None,
        "workers_used": [],
        "forge_attempt": None,
        "failures": [],
        "chunk_failures": [],
        "started_at": time.time(),
        "finished_at": None,
        "requested_size": len(paths),
        "inventory_rows": len(rows),
        "missing_paths": missing,
    }

    if try_forge:
        ok, msg = try_forge_batch(batch_id, instruction, len(rows))
        record["forge_attempt"] = {"ok": ok, "message": msg}

    chunks: List[List[Dict[str, Any]]] = [
        rows[i : i + chunk_size] for i in range(0, len(rows), chunk_size)
    ]
    all_results: List[Dict[str, Any]] = []
    workers_used: List[str] = []

    for ci, chunk in enumerate(chunks):
        label = f"{batch_id} chunk {ci+1}/{len(chunks)}"
        parsed, used_for_chunk, fails = process_rows_adaptive(
            workers,
            (worker_idx + ci) % len(workers),
            instruction,
            chunk,
            label,
            min_chunk_size,
        )
        if fails:
            record["chunk_failures"].append({"chunk": ci, "errors": fails})
        if not parsed:
            record["status"] = "failed"
            record["failures"].append(f"chunk {ci} exhausted workers")
            continue
        for wid in used_for_chunk:
            if wid and wid not in workers_used:
                workers_used.append(wid)
        for item in parsed:
            if isinstance(item, dict) and item.get("path"):
                all_results.append(item)

    all_results.sort(key=lambda x: (-int(x.get("usefulness_1_to_5", 0) or 0), x.get("path", "")))
    result_doc = {
        "batch_id": batch_id,
        "mode": "fleet_deep_llm",
        "requested_size": len(paths),
        "processed_size": len(all_results),
        "instruction": instruction,
        "workers_used": workers_used,
        "results": all_results,
    }
    out_path = out_dir / f"{batch_id}.json"
    out_path.write_text(json.dumps(result_doc, indent=2), encoding="utf-8")
    record["out_file"] = str(out_path)
    record["processed_size"] = len(all_results)
    record["workers_used"] = workers_used
    record["worker_primary"] = workers_used[0] if workers_used else None
    record["finished_at"] = time.time()
    if record["status"] != "failed":
        record["status"] = "ok" if len(all_results) >= max(1, len(rows) // 2) else "partial"
    return record


def save_manifest(path: Path, manifest: Dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(manifest, indent=2), encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description="Fleet dispatch for blueprint audit LLM batches")
    ap.add_argument("--repo", type=Path, default=Path(__file__).resolve().parents[1])
    ap.add_argument("--batches", type=Path, default=None)
    ap.add_argument("--inventory", type=Path, default=None)
    ap.add_argument("--out-dir", type=Path, default=None)
    ap.add_argument("--parallel", type=int, default=3, help="Concurrent batches")
    ap.add_argument("--try-forge", action="store_true", help="Ping Forge /send before workers")
    ap.add_argument("--batch-ids", nargs="*", help="Only these batch ids")
    ap.add_argument("--skip-done", action="store_true", default=True)
    ap.add_argument("--chunk-size", type=int, default=DEFAULT_CHUNK_SIZE)
    ap.add_argument("--min-chunk-size", type=int, default=DEFAULT_MIN_CHUNK_SIZE)
    ap.add_argument("--allow-model-regex", default="(gemma|qwen|r1|deepseek)")
    ap.add_argument("--deny-model-regex", default="(cpu|tiny|mini)")
    ap.add_argument("--min-model-size-b", type=int, default=14)
    args = ap.parse_args()

    repo = args.repo
    audit = repo / "reports" / "blueprint_audit"
    batches_path = args.batches or audit / "llm_dispatch_batches.json"
    inventory_path = args.inventory or audit / "file_inventory.jsonl"
    out_dir = args.out_dir or audit / "llm_results" / "deep"
    manifest_path = audit / "llm_results" / "fleet_dispatch_manifest.json"

    batches: List[Dict[str, Any]] = json.loads(batches_path.read_text(encoding="utf-8"))
    if args.batch_ids:
        want = set(args.batch_ids)
        batches = [b for b in batches if b.get("batch_id") in want]

    inventory = load_jsonl(inventory_path)
    worker_defs = [Worker(**w) for w in DEFAULT_WORKERS]
    filtered_workers, worker_filter_errors = filter_workers(
        worker_defs,
        args.allow_model_regex,
        args.deny_model_regex,
        args.min_model_size_b,
    )
    workers = [ws.worker for ws in filtered_workers]
    worker_models = [{"id": ws.worker.id, "model": ws.model_id, "score_b": ws.model_score} for ws in filtered_workers]
    if not workers:
        print("No healthy/capable workers after model filtering", file=sys.stderr)
        for err in worker_filter_errors:
            print(err, file=sys.stderr)
        return 2

    manifest: Dict[str, Any] = {
        "status": "running",
        "started_at": time.time(),
        "workers": [w.id for w in workers],
        "worker_models": worker_models,
        "worker_filter_errors": worker_filter_errors,
        "chunk_size": args.chunk_size,
        "min_chunk_size": args.min_chunk_size,
        "batches": [],
        "summary": {},
    }
    if manifest_path.exists() and args.skip_done:
        try:
            prev = json.loads(manifest_path.read_text(encoding="utf-8"))
            done_ids = {
                b["batch_id"]
                for b in prev.get("batches", [])
                if b.get("status") == "ok" and b.get("processed_size", 0) > 0
            }
            batches = [b for b in batches if b.get("batch_id") not in done_ids]
            if done_ids:
                manifest["skipped_already_ok"] = sorted(done_ids)
        except Exception:
            pass

    save_manifest(manifest_path, manifest)

    def run_one(idx_batch: Tuple[int, Dict[str, Any]]) -> Dict[str, Any]:
        idx, batch = idx_batch
        return process_batch(
            batch,
            inventory,
            workers,
            idx % len(workers),
            args.try_forge,
            out_dir,
            max(1, args.chunk_size),
            max(1, args.min_chunk_size),
        )

    results: List[Dict[str, Any]] = []
    with ThreadPoolExecutor(max_workers=max(1, args.parallel)) as ex:
        futs = {ex.submit(run_one, (i, b)): b for i, b in enumerate(batches)}
        for fut in as_completed(futs):
            rec = fut.result()
            results.append(rec)
            manifest["batches"] = sorted(
                results, key=lambda x: x.get("batch_id", "")
            )
            ok = sum(1 for r in results if r.get("status") == "ok")
            partial = sum(1 for r in results if r.get("status") == "partial")
            failed = sum(1 for r in results if r.get("status") == "failed")
            manifest["summary"] = {"ok": ok, "partial": partial, "failed": failed, "total": len(results)}
            save_manifest(manifest_path, manifest)

    manifest["status"] = "done"
    manifest["finished_at"] = time.time()
    manifest["batches"] = sorted(results, key=lambda x: x.get("batch_id", ""))
    save_manifest(manifest_path, manifest)

    failed = [r for r in results if r.get("status") == "failed"]
    if failed:
        print(f"FAILED batches: {[r['batch_id'] for r in failed]}", file=sys.stderr)
        return 1
    print(f"Fleet dispatch complete: {manifest['summary']}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
