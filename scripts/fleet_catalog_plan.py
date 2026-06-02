#!/usr/bin/env python3
"""
Phase 3–4: Consolidation plan + vector ingest JSONL from fleet_master.jsonl.

Uses examine_blob (deep for RS). Optional Jina on P106 via --embed.
"""
from __future__ import annotations

import argparse
import json
import os
import sys
import time
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
CLEAN_REPOS = Path(os.environ.get("CLEAN_REPOS", REPO / "var/recovery/clean_repos"))

sys.path.insert(0, str(REPO / "scripts/recovery"))
from p106_drive_map import (  # noqa: E402
    build_repo_profiles,
    heuristic_junk,
    load_embedder,
    match_repo,
)

RS_MODULE_KEYWORDS = {
    "cesarops-detection": ["detection", "tile", "glint", "process_tile", "tpu", "movidius"],
    "wreckhunter2000-1": ["wreck", "sonar", "geotiff", "lake", "scan", "census"],
    "sonarsniffer": ["sonar", "channel", "sniffer"],
    "universal_downloader": ["stac", "sentinel", "landsat", "download", "earthdata"],
}


def log(msg: str) -> None:
    print(f"[fleet-plan] {msg}", flush=True)


def load_master(path: Path) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    with path.open(encoding="utf-8") as f:
        for line in f:
            if line.strip():
                rows.append(json.loads(line))
    return rows


def rs_target_hint(row: dict[str, Any]) -> str:
    prof = row.get("rs_profile") or {}
    blob = (row.get("examine_blob") or "").lower()
    path = (row.get("path") or "").lower()
    combined = blob + " " + path + " " + json.dumps(prof, default=str).lower()
    scores: dict[str, int] = {}
    for repo, kws in RS_MODULE_KEYWORDS.items():
        scores[repo] = sum(1 for k in kws if k in combined)
    if not scores:
        return "data-archive"
    best = max(scores.items(), key=lambda x: x[1])
    return best[0] if best[1] > 0 else "data-archive"


def plan_action(row: dict[str, Any], category: str, conf: float) -> dict[str, Any]:
    path = row.get("path", "")
    asset = row.get("asset_class", "")
    action = "review"
    target_repo = "unknown"
    target_path = ""
    notes: list[str] = []

    if row.get("duplicate_count", 0) > 0:
        notes.append(f"duplicates={row.get('duplicate_count')}")

    if asset == "remote_sensing":
        target_repo = rs_target_hint(row)
        sensor = row.get("sensor_guess") or (row.get("rs_profile") or {}).get("sensor_guess")
        if conf >= 0.6 or target_repo != "data-archive":
            action = "catalog_data"
            notes.append(f"sensor={sensor}")
            notes.append("deep_profile=yes")
        else:
            action = "review_rs"
        notes.append("do_not_delete_without_gdal_check")
        return {
            "path": path,
            "sha256": row.get("sha256"),
            "node": row.get("node"),
            "host": row.get("host"),
            "asset_class": asset,
            "category": category,
            "confidence": conf,
            "action": action,
            "target_repo": target_repo,
            "target_path": target_path,
            "sensor_guess": sensor,
            "notes": "; ".join(notes),
            "examine_depth": row.get("examine_depth"),
        }

    if category.startswith("MATCHES_REPO_"):
        target_repo = category.replace("MATCHES_REPO_", "", 1)
        action = "move_to_repo"
    elif category.startswith("LIKELY_JUNK") or category.startswith("JUNK_"):
        action = "quarantine"
    elif category == "VALID_CODE_ORPHAN":
        action = "review"
    elif category == "REVIEW_HEURISTIC_OK":
        action = "review"

    return {
        "path": path,
        "sha256": row.get("sha256"),
        "node": row.get("node"),
        "host": row.get("host"),
        "asset_class": asset,
        "category": category,
        "confidence": conf,
        "action": action,
        "target_repo": target_repo,
        "target_path": target_path,
        "notes": "; ".join(notes),
        "examine_depth": row.get("examine_depth"),
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--master", default="")
    ap.add_argument("--out-dir", default="")
    ap.add_argument("--embed", action="store_true")
    ap.add_argument("--device", default="0")
    ap.add_argument("--max-rows", type=int, default=0)
    args = ap.parse_args()

    catalog_dir = REPO / "var/fleet-catalog"
    master_path = Path(args.master) if args.master else catalog_dir / "fleet_master.jsonl"
    out_dir = Path(args.out_dir) if args.out_dir else catalog_dir
    out_dir.mkdir(parents=True, exist_ok=True)

    if not master_path.is_file():
        log(f"missing {master_path} — run fleet_file_catalog + merge first")
        return 1

    rows = load_master(master_path)
    if args.max_rows:
        rows = rows[: args.max_rows]

    embedder = None
    profiles: dict[str, Any] = {}
    if args.embed:
        embedder = load_embedder(f"cuda:{args.device}")
        profiles = build_repo_profiles(embedder, CLEAN_REPOS)

    plan: list[dict[str, Any]] = []
    ingest: list[dict[str, Any]] = []
    t0 = time.time()

    for i, row in enumerate(rows):
        blob = row.get("examine_blob") or row.get("text_snippet") or ""
        path = row.get("path", "")
        asset = row.get("asset_class", "")

        if asset == "remote_sensing":
            category = f"REMOTE_SENSING_{rs_target_hint(row).upper()}"
            conf = 0.75 if row.get("rs_profile", {}).get("gdalinfo") else 0.55
        elif embedder and profiles and len(blob) > 80:
            try:
                vec = embedder.encode(blob[:12000], convert_to_numpy=True)
                category, conf = match_repo(vec, profiles, 0.58)
            except Exception:
                category, conf = "ERROR_EMBED", 0.0
        else:
            is_junk, cat, conf = heuristic_junk(Path(path), blob[:4096])
            category = cat if is_junk else "REVIEW_HEURISTIC_OK"
            if not is_junk:
                conf = 0.5

        entry = plan_action(row, category, float(conf))
        plan.append(entry)
        if blob and entry["action"] not in ("quarantine",):
            ingest.append(
                {
                    "path": path,
                    "repo": entry.get("target_repo"),
                    "category": category,
                    "action": entry["action"],
                    "confidence": conf,
                    "asset_class": asset,
                    "snippet": blob[:16000],
                }
            )
        if (i + 1) % 500 == 0:
            log(f"planned {i + 1}/{len(rows)}")

    plan_path = out_dir / "consolidation_plan.json"
    plan_path.write_text(json.dumps(plan, indent=2), encoding="utf-8")
    ingest_path = out_dir / "nautivecs_ingest.jsonl"
    with ingest_path.open("w", encoding="utf-8") as f:
        for line in ingest:
            f.write(json.dumps(line, ensure_ascii=False) + "\n")

    from collections import Counter

    actions = Counter(p["action"] for p in plan)
    summary = {
        "rows": len(plan),
        "elapsed_s": round(time.time() - t0, 1),
        "actions": dict(actions),
        "ingest_lines": len(ingest),
    }
    (out_dir / "plan_summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    log(f"plan -> {plan_path} ({len(plan)} entries)")
    log(f"ingest -> {ingest_path} ({len(ingest)} lines)")
    log(f"actions: {dict(actions)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
