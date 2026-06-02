#!/usr/bin/env python3
"""
Phase 2: Merge per-node fleet catalogs, dedup by sha256, flag RS priority rows.
"""
from __future__ import annotations

import argparse
import json
import os
from collections import defaultdict
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))


def log(msg: str) -> None:
    print(f"[fleet-merge] {msg}", flush=True)


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    if not path.is_file():
        return rows
    with path.open(encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line:
                rows.append(json.loads(line))
    return rows


def merge_catalogs(catalog_dir: Path, preferred_host: str) -> tuple[list[dict[str, Any]], dict[str, Any]]:
    skip_names = {
        "fleet_master.jsonl",
        "fleet_master_deep.jsonl",
        "fleet_master_summarized.jsonl",
        "nautivecs_ingest.jsonl",
        "rs_llm_summaries.jsonl",
    }
    files = sorted(catalog_dir.glob("*.jsonl"))
    all_rows: list[dict[str, Any]] = []
    for fp in files:
        if fp.name in skip_names or fp.name.startswith("batch_"):
            continue
        all_rows.extend(load_jsonl(fp))

    by_hash: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in all_rows:
        h = row.get("sha256")
        if h:
            by_hash[h].append(row)

    merged: list[dict[str, Any]] = []
    dup_count = 0
    rs_count = 0

    for _h, group in by_hash.items():
        if len(group) > 1:
            dup_count += len(group) - 1
        def rank(r: dict[str, Any]) -> tuple:
            path = r.get("path", "")
            host_pref = 0 if preferred_host in str(r.get("host", "")).lower() else 1
            nfs_pref = 0 if "/codebase/repos/" in path else 1
            return (host_pref, nfs_pref, len(path))

        group.sort(key=rank)
        canon = dict(group[0])
        canon["duplicate_paths"] = [g["path"] for g in group[1:][:20]]
        canon["duplicate_count"] = len(group) - 1
        if canon.get("asset_class") == "remote_sensing":
            rs_count += 1
            canon["consolidation_priority"] = "high"
        merged.append(canon)

    no_hash = [r for r in all_rows if not r.get("sha256")]
    merged.extend(no_hash)

    stats = {
        "input_rows": len(all_rows),
        "unique_sha256": len(by_hash),
        "duplicate_paths_collapsed": dup_count,
        "remote_sensing_unique": rs_count,
        "no_hash_rows": len(no_hash),
    }
    return merged, stats


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--catalog-dir", default="")
    ap.add_argument("--config", default="")
    args = ap.parse_args()

    cfg_path = Path(args.config) if args.config else REPO / "config/fleet_catalog_roots.json"
    cfg = json.loads(cfg_path.read_text(encoding="utf-8"))
    catalog_dir = Path(args.catalog_dir) if args.catalog_dir else REPO / cfg.get("catalog_out", "var/fleet-catalog")
    preferred = cfg.get("preferred_canonical_host", "t440")

    deep_master = catalog_dir / "fleet_master_deep.jsonl"
    master = catalog_dir / "fleet_master.jsonl"
    merged, stats = merge_catalogs(catalog_dir, preferred)
    if deep_master.is_file():
        by_path = {r["path"]: r for r in load_jsonl(deep_master) if r.get("path")}
        overlay = 0
        for i, row in enumerate(merged):
            p = row.get("path")
            if p and p in by_path:
                merged[i] = by_path[p]
                overlay += 1
        stats["deep_overlay"] = overlay
        stats["deep_source"] = str(deep_master)
    with master.open("w", encoding="utf-8") as f:
        for row in merged:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    stats_path = catalog_dir / "merge_summary.json"
    stats_path.write_text(json.dumps(stats, indent=2), encoding="utf-8")
    log(f"merged {len(merged)} rows -> {master}")
    log(f"stats: {stats}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
