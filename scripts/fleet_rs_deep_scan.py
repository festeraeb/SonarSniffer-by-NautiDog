#!/usr/bin/env python3
"""
Parallel deep RS scan — runs immediately after fast catalog marks remote_sensing.

Workers (from config rs_deep_workers):
  - Local process pools on cesarops2 / t440
  - Optional SSH remote pools on peer nodes

Each worker runs GDAL/sidecar/header deep profile (NOT llama — keep GPUs free for plan/embed).
"""
from __future__ import annotations

import argparse
import json
import os
import socket
import subprocess
import sys
import time
from concurrent.futures import ProcessPoolExecutor, as_completed
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
CONFIG_PATH = Path(os.environ.get("FLEET_CATALOG_CONFIG", REPO / "config/fleet_catalog_roots.json"))

sys.path.insert(0, str(REPO / "scripts/recovery"))
from rs_deep_profile import apply_deep_to_row, shallow_profile  # noqa: E402


def log(msg: str) -> None:
    print(f"[rs-deep] {msg}", flush=True)


def load_config() -> dict[str, Any]:
    return json.loads(CONFIG_PATH.read_text(encoding="utf-8"))


def load_pending_rows(sources: list[Path]) -> list[dict[str, Any]]:
    pending: list[dict[str, Any]] = []
    for src in sources:
        if not src.is_file():
            continue
        with src.open(encoding="utf-8") as f:
            for line in f:
                if not line.strip():
                    continue
                row = json.loads(line)
                if row.get("asset_class") != "remote_sensing":
                    continue
                st = row.get("deep_scan_status", "")
                if st == "complete" and row.get("rs_profile", {}).get("gdalinfo"):
                    continue
                if st == "complete" and row.get("examine_blob") and len(row.get("examine_blob", "")) > 500:
                    continue
                pending.append(row)
    return pending


def _worker_deep_one(payload: dict[str, Any]) -> dict[str, Any]:
    """Process-pool entry: deep profile one file."""
    path = Path(payload["path"])
    rs_cfg = payload["rs_cfg"]
    worker_id = payload.get("worker_id", "local")
    row = payload["row"]
    os.environ["RS_DEEP_WORKER_ID"] = worker_id
    try:
        if not path.is_file():
            row["deep_scan_status"] = "error"
            row["deep_scan_error"] = "file_missing"
            return row
        return apply_deep_to_row(row, path, rs_cfg)
    except Exception as e:
        row["deep_scan_status"] = "error"
        row["deep_scan_error"] = str(e)[:500]
        return row


def run_local_pool(
    batch: list[dict[str, Any]],
    rs_cfg: dict[str, Any],
    worker_id: str,
    parallel: int,
) -> list[dict[str, Any]]:
    jobs = [
        {"path": r["path"], "row": r, "rs_cfg": rs_cfg, "worker_id": worker_id}
        for r in batch
    ]
    out: list[dict[str, Any]] = []
    with ProcessPoolExecutor(max_workers=max(1, parallel)) as ex:
        futs = [ex.submit(_worker_deep_one, j) for j in jobs]
        for fut in as_completed(futs):
            out.append(fut.result())
    return out


def _resolve_ssh_target(target: str) -> str:
    aliases = {
        "10.0.0.61": "t440",
        "10.0.0.62": "t440-62",
        "cesarops@10.0.0.61": "t440",
        "cesarops@10.0.0.62": "t440-62",
    }
    return aliases.get(target, target)


def _map_path_for_node(path: str, node_cfg: dict[str, Any]) -> str:
    path_map = node_cfg.get("path_map") or {}
    for src, dst in sorted(path_map.items(), key=lambda x: -len(x[0])):
        if path.startswith(src):
            return dst + path[len(src) :]
    return path


def _ssh_batch_rows(batch: list[dict[str, Any]], node_cfg: dict[str, Any]) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    for row in batch:
        r = dict(row)
        r["path"] = _map_path_for_node(r.get("path", ""), node_cfg)
        r["_catalog_path"] = row.get("path", "")
        out.append(r)
    return out


def run_ssh_pool(
    batch: list[dict[str, Any]],
    ssh_target: str,
    worker_id: str,
    parallel: int,
    rs_cfg: dict[str, Any],
    scratch_dir: Path,
    remote_repo: str | None = None,
    path_map_node: str = "t440",
) -> list[dict[str, Any]]:
    """Ship path list to remote host; remote runs same script --worker-local."""
    if not batch:
        return []
    cfg = load_config()
    node_cfg = cfg.get("nodes", {}).get(path_map_node, {})
    remote_repo = remote_repo or node_cfg.get("remote_repo") or str(REPO)
    resolved = _resolve_ssh_target(ssh_target)
    mapped = _ssh_batch_rows(batch, node_cfg)
    scratch_dir.mkdir(parents=True, exist_ok=True)
    batch_file = scratch_dir / f"batch_{worker_id}_{int(time.time())}.jsonl"
    with batch_file.open("w", encoding="utf-8") as f:
        for row in mapped:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")
    remote_in = f"/tmp/fleet_rs_deep_{worker_id}.jsonl"
    remote_out = f"/tmp/fleet_rs_deep_{worker_id}_out.jsonl"
    subprocess.run(
        ["scp", "-o", "BatchMode=yes", "-q", str(batch_file), f"{resolved}:{remote_in}"],
        check=True,
        timeout=120,
    )
    remote_cmd = (
        f"REPO='{remote_repo}' RS_DEEP_WORKER_ID='{worker_id}' "
        f"python3 '{remote_repo}/scripts/fleet_rs_deep_scan.py' "
        f"--worker-local --input '{remote_in}' --output '{remote_out}' --parallel {parallel}"
    )
    subprocess.run(
        ["ssh", "-o", "BatchMode=yes", "-o", "ConnectTimeout=15", resolved, remote_cmd],
        check=True,
        timeout=3600,
    )
    local_out = scratch_dir / f"out_{worker_id}.jsonl"
    subprocess.run(
        ["scp", "-o", "BatchMode=yes", "-q", f"{resolved}:{remote_out}", str(local_out)],
        check=True,
        timeout=120,
    )
    by_mapped = {r["path"]: r.get("_catalog_path", r["path"]) for r in mapped}
    results = load_pending_rows([local_out])
    for row in results:
        orig = by_mapped.get(row.get("path", ""))
        if orig:
            row["path"] = orig
        row.pop("_catalog_path", None)
    return results


def shard_batches(rows: list[dict[str, Any]], workers: list[dict[str, Any]]) -> list[tuple[dict[str, Any], list[dict[str, Any]]]]:
    if not workers:
        return []
    total_slots = sum(max(1, int(w.get("parallel", 4))) for w in workers)
    shards: list[list[dict[str, Any]]] = [[] for _ in workers]
    for i, row in enumerate(rows):
        shards[i % len(workers)].append(row)
    return [(workers[i], shards[i]) for i in range(len(workers)) if shards[i]]


def write_results(results: list[dict[str, Any]], out_dir: Path) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)
    for row in results:
        h = row.get("sha256") or str(hash(row.get("path", "")))
        key = h[:16]
        (out_dir / f"{key}.json").write_text(json.dumps(row, indent=2), encoding="utf-8")


def patch_jsonl_sources(sources: list[Path], results: list[dict[str, Any]], out_path: Path) -> None:
    by_path = {r["path"]: r for r in results}
    merged_lines: list[str] = []
    for src in sources:
        if not src.is_file():
            continue
        with src.open(encoding="utf-8") as f:
            for line in f:
                if not line.strip():
                    continue
                row = json.loads(line)
                p = row.get("path")
                if p in by_path:
                    row = by_path[p]
                merged_lines.append(json.dumps(row, ensure_ascii=False))
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text("\n".join(merged_lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--input", default="", help="jsonl with pending RS rows")
    ap.add_argument("--output", default="", help="output jsonl (worker-local mode)")
    ap.add_argument("--worker-local", action="store_true", help="Run on this host only (ssh remote)")
    ap.add_argument("--parallel", type=int, default=0)
    ap.add_argument("--catalog-dir", default="")
    ap.add_argument("--max-rows", type=int, default=0)
    args = ap.parse_args()

    cfg = load_config()
    rs_cfg = cfg.get("remote_sensing", {})
    catalog_dir = Path(args.catalog_dir) if args.catalog_dir else REPO / cfg.get("catalog_out", "var/fleet-catalog")

    if args.worker_local:
        src = Path(args.input)
        out = Path(args.output)
        rows = load_pending_rows([src])
        if args.max_rows:
            rows = rows[: args.max_rows]
        wid = os.environ.get("RS_DEEP_WORKER_ID", "ssh-worker")
        par = args.parallel or max(2, (os.cpu_count() or 4) // 2)
        log(f"worker-local {wid} parallel={par} rows={len(rows)}")
        done = run_local_pool(rows, rs_cfg, wid, par)
        out.parent.mkdir(parents=True, exist_ok=True)
        with out.open("w", encoding="utf-8") as f:
            for row in done:
                f.write(json.dumps(row, ensure_ascii=False) + "\n")
        log(f"wrote {len(done)} -> {out}")
        return 0

    sources = []
    if args.input:
        sources.append(Path(args.input))
    else:
        sources.extend(sorted(catalog_dir.glob("*.jsonl")))
        master = catalog_dir / "fleet_master.jsonl"
        if master.is_file():
            sources = [master]

    pending = load_pending_rows(sources)
    if args.max_rows:
        pending = pending[: args.max_rows]
    if not pending:
        log("no pending RS rows")
        return 0

    workers = cfg.get("rs_deep_workers") or [
        {"id": "local-cpu", "host": "local", "parallel": max(4, (os.cpu_count() or 8) // 2)},
        {"id": "p106-slot", "host": "local", "parallel": 2, "cuda_device": "0", "role": "p106"},
    ]
    log(f"pending RS files: {len(pending)} workers: {len(workers)}")

    sharded = shard_batches(pending, workers)
    all_results: list[dict[str, Any]] = []
    scratch = catalog_dir / "rs_deep_scratch"

    for worker, batch in sharded:
        wid = worker.get("id", "worker")
        par = int(worker.get("parallel", 4))
        host = worker.get("host", "local")
        log(f"  worker {wid} host={host} parallel={par} batch={len(batch)}")
        if host == "ssh" and worker.get("ssh"):
            try:
                all_results.extend(
                    run_ssh_pool(
                        batch,
                        worker["ssh"],
                        wid,
                        par,
                        rs_cfg,
                        scratch,
                        remote_repo=worker.get("remote_repo"),
                        path_map_node=worker.get("path_map_node", "t440"),
                    )
                )
            except (subprocess.CalledProcessError, OSError) as e:
                log(f"  warn: ssh worker {wid} failed ({e}); re-queue on local CPU")
                all_results.extend(run_local_pool(batch, rs_cfg, f"{wid}-fallback", par))
        else:
            if worker.get("cuda_device") is not None:
                os.environ["CUDA_VISIBLE_DEVICES"] = str(worker["cuda_device"])
            all_results.extend(run_local_pool(batch, rs_cfg, wid, par))

    deep_dir = catalog_dir / "rs_deep"
    write_results(all_results, deep_dir)

    patched = catalog_dir / "fleet_master_deep.jsonl"
    patch_jsonl_sources(sources, all_results, patched)
    log(f"patched catalog -> {patched}")
    log(f"per-file results -> {deep_dir}/")
    ok = sum(1 for r in all_results if r.get("deep_scan_status") == "complete")
    log(f"complete={ok}/{len(all_results)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
