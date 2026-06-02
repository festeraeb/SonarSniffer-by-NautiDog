#!/usr/bin/env python3
"""
Phase 1: Fleet file catalog — walk configured roots per node.

Remote-sensing files get deep rs_profile (GDAL, sidecars, headers).
Code/text get larger snippets than a 3-line skim.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import socket
import sys
import time
from pathlib import Path
from typing import Any, Iterator

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
CONFIG_PATH = Path(os.environ.get("FLEET_CATALOG_CONFIG", REPO / "config/fleet_catalog_roots.json"))

sys.path.insert(0, str(REPO / "scripts/recovery"))
from rs_deep_profile import (  # noqa: E402
    RS_PATH_RE,
    deep_profile,
    is_remote_sensing_path,
    sensor_from_path,
    shallow_profile,
)


def log(msg: str) -> None:
    print(f"[fleet-catalog] {msg}", flush=True)


def load_config() -> dict[str, Any]:
    return json.loads(CONFIG_PATH.read_text(encoding="utf-8"))


def hostname_node() -> str:
    h = socket.gethostname().split(".")[0].lower()
    if "t440" in h:
        return "t440"
    if "nautik" in h:
        return "nautik9"
    return "cesarops2"


def should_skip_dir(parts: tuple[str, ...], skip_names: set[str]) -> bool:
    return any(p in skip_names for p in parts)


def file_sha256(path: Path, max_bytes: int) -> str | None:
    try:
        size = path.stat().st_size
        h = hashlib.sha256()
        with path.open("rb") as f:
            if size <= max_bytes:
                for chunk in iter(lambda: f.read(65536), b""):
                    h.update(chunk)
            else:
                # large file: hash first+last 1MB + size for speed
                head = f.read(1_048_576)
                h.update(head)
                h.update(str(size).encode())
                if size > 2_097_152:
                    f.seek(-1_048_576, os.SEEK_END)
                    h.update(f.read(1_048_576))
        return h.hexdigest()
    except OSError:
        return None


def classify_asset(path: Path, cfg: dict[str, Any]) -> str:
    if is_remote_sensing_path(path):
        return "remote_sensing"
    ext = path.suffix.lower()
    if ext in set(cfg.get("text_code", {}).get("extensions", [])):
        return "text_code"
    if ext in {".gguf", ".bin", ".pt", ".onnx", ".zip", ".tar", ".gz", ".7z"}:
        return "model_or_archive"
    return "other"


def examine_text(path: Path, limit: int, asset: str) -> str:
    try:
        size = path.stat().st_size
        cap = limit
        if asset == "text_code" and size < 131072:
            cap = max(limit, size)  # read whole small source files
        with path.open("r", encoding="utf-8", errors="ignore") as f:
            return f.read(cap)
    except OSError:
        return ""


def catalog_row(
    path: Path,
    node: str,
    host: str,
    cfg: dict[str, Any],
    defer_rs_deep: bool = False,
) -> dict[str, Any]:
    rs_cfg = cfg.get("remote_sensing", {})
    text_cfg = cfg.get("text_code", {})
    hash_max = int(cfg.get("hash_max_bytes", 52_428_800))
    try:
        st = path.stat()
    except OSError:
        return {}

    asset = classify_asset(path, cfg)
    row: dict[str, Any] = {
        "path": str(path.resolve()),
        "node": node,
        "host": host,
        "asset_class": asset,
        "ext": path.suffix.lower(),
        "size_bytes": st.st_size,
        "mtime": int(st.st_mtime),
        "sha256": file_sha256(path, hash_max),
    }

    if asset == "remote_sensing":
        if defer_rs_deep:
            prof = shallow_profile(path)
            row["rs_profile"] = prof
            row["sensor_guess"] = prof.get("sensor_guess")
            row["examine_depth"] = "shallow"
            row["deep_scan_status"] = "pending"
            row["examine_blob"] = (
                f"PATH: {path}\nSENSOR_GUESS: {row['sensor_guess']}\n"
                f"HLS: {json.dumps(prof.get('hls_parse') or {})}\n"
                f"STATUS: pending_parallel_deep_scan\n"
            )
        else:
            prof, blob = deep_profile(path, rs_cfg)
            row["rs_profile"] = prof
            row["examine_blob"] = blob
            row["sensor_guess"] = prof.get("sensor_guess") or sensor_from_path(path)
            row["examine_depth"] = "deep"
            row["deep_scan_status"] = "complete"
    elif asset == "text_code":
        limit = int(text_cfg.get("default_snippet_bytes", 8192))
        if rs_path_hint(path):
            limit = int(rs_cfg.get("max_text_examine_bytes", 65536))
        row["text_snippet"] = examine_text(path, limit, asset)
        row["examine_blob"] = f"PATH: {path}\n\n{row['text_snippet']}"
        row["examine_depth"] = "extended" if limit > 8192 else "standard"
    else:
        row["examine_depth"] = "metadata_only"

    return row


def rs_path_hint(path: Path) -> bool:
    return bool(RS_PATH_RE.search(str(path)))


def iter_files(roots: list[str], skip_names: set[str]) -> Iterator[Path]:
    for root_s in roots:
        root = Path(root_s)
        if not root.is_dir():
            log(f"skip missing root: {root}")
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            parts = Path(dirpath).parts
            dirnames[:] = [d for d in dirnames if d not in skip_names]
            if should_skip_dir(parts, skip_names):
                continue
            for fn in filenames:
                yield Path(dirpath) / fn


def run_catalog(
    node: str,
    roots: list[str],
    out_path: Path,
    cfg: dict[str, Any],
    max_files: int,
    defer_rs_deep: bool = False,
) -> dict[str, int]:
    host = socket.gethostname()
    skip = set(cfg.get("skip_dir_names", []))
    counts: dict[str, int] = {}
    n = 0
    out_path.parent.mkdir(parents=True, exist_ok=True)

    with out_path.open("w", encoding="utf-8") as out:
        for path in iter_files(roots, skip):
            if max_files and n >= max_files:
                break
            row = catalog_row(path, node, host, cfg, defer_rs_deep=defer_rs_deep)
            if not row:
                continue
            out.write(json.dumps(row, ensure_ascii=False) + "\n")
            ac = row.get("asset_class", "other")
            counts[ac] = counts.get(ac, 0) + 1
            n += 1
            if n % 2000 == 0:
                log(f"  {n} files...")

    return {"total": n, **counts}


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--node", default="", help="Config node id (default: autodetect)")
    ap.add_argument("--out", default="", help="Output jsonl path")
    ap.add_argument("--max-files", type=int, default=0)
    ap.add_argument("--roots", nargs="*", help="Override roots")
    ap.add_argument(
        "--defer-rs-deep",
        action="store_true",
        help="Tag RS files pending; run fleet_rs_deep_scan.py in parallel next",
    )
    args = ap.parse_args()

    cfg = load_config()
    node = args.node or hostname_node()
    nodes = cfg.get("nodes", {})
    if node not in nodes:
        log(f"unknown node {node}; available: {list(nodes.keys())}")
        return 1

    roots = args.roots or nodes[node].get("roots", [])
    roots = [r for r in roots if Path(r).is_dir()]
    if not roots:
        log(f"no valid roots for {node}")
        return 1

    out_dir = REPO / cfg.get("catalog_out", "var/fleet-catalog")
    out_path = Path(args.out) if args.out else out_dir / f"{node}.jsonl"

    log(f"catalog node={node} roots={len(roots)} -> {out_path}")
    t0 = time.time()
    counts = run_catalog(node, roots, out_path, cfg, args.max_files, defer_rs_deep=args.defer_rs_deep)
    summary = {
        "node": node,
        "out": str(out_path),
        "elapsed_s": round(time.time() - t0, 1),
        "counts": counts,
    }
    (out_dir / f"{node}.summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    log(f"done {counts} ({summary['elapsed_s']}s)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
