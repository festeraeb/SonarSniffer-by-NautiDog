#!/usr/bin/env python3
"""Merge Gemma + Qwen integrate_out plans → prioritized Rust port queue."""
from __future__ import annotations

import json
import re
from pathlib import Path

REPO = Path("/codebase/repos/wreckhunter2000-1")
OUT_DIRS = [
    REPO / "integrate_out" / "p1000",
    REPO / "integrate_out" / "p1001",
    REPO / "integrate_out" / "qwen_moe",
    REPO / "integrate_out" / "split_rust",
    REPO / "integrate_out",
]
QUEUE = REPO / "integrate" / "RUST_PORT_QUEUE.json"

AGENT_PORTED = {
    "analyze_fuel_leaks.py",
    "cesarops_engine.py",
    "prioritized_pull_v2.py",
    "multi_sensor_scan.py",
    "lake_michigan_full_scan.py",
    "cuda_direct.py",
    "integrated_forensic_scan.py",
    "full_basin_scan.py",
    "triple_lock_fusion.py",
    "daily_satellite_pull.py",
    "cesarops_cli.py",
    "cesarops_agent_gui.py",
    "ai_director.py",
    "analyze_crossref.py",
    "drive_identity.py",
    "dynamic_db_key.py",
    # Agent + split-rust hard ports (Rust modules in cesarops-inference/src/integrate/)
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
    "andaste_geometry_test.py",
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


def parse_md(path: Path) -> dict | None:
    text = path.read_text(encoding="utf-8", errors="replace")
    if len(text) < 80:
        return None
    m = re.search(r"^## Verdict\s*\n(\S+)", text, re.M)
    if not m:
        return None
    verdict = m.group(1).strip()
    tm = re.search(r"^## Target path\s*\n(.+)", text, re.M)
    target = tm.group(1).strip() if tm else ""
    src = ""
    if path.name.startswith("int__"):
        src = path.name.replace("int__", "").replace(".md", "") + ".py"
    elif path.name.startswith("rust__"):
        src = path.name.replace("rust__", "").replace(".md", "") + ".py"
    else:
        return None
    rust_path = ""
    rm = re.search(r"^## Rust path\s*\n(.+)", text, re.M)
    if rm:
        rust_path = rm.group(1).strip()
    return {
        "verdict": verdict,
        "target": target,
        "source_py": src,
        "plan_file": str(path),
        "rust_path": rust_path,
        "rust_status": "ported" if src in AGENT_PORTED else "pending",
    }


def main() -> None:
    existing: dict[str, dict] = {}
    if QUEUE.is_file():
        for row in json.loads(QUEUE.read_text(encoding="utf-8")):
            st = row.get("rust_status", "")
            if st in ("merged", "archived", "keep_live") or row.get("apply_note"):
                existing[row["source_py"]] = row

    by_src: dict[str, dict] = {}
    for root in OUT_DIRS:
        if not root.is_dir():
            continue
        for md in root.rglob("*.md"):
            if not (md.name.startswith("int__") or md.name.startswith("rust__")):
                continue
            row = parse_md(md)
            if not row or not row["source_py"]:
                continue
            key = row["source_py"]
            prev = by_src.get(key)
            # Prefer entry with non-empty target; tie-break longer plan path
            if not prev:
                by_src[key] = row
            elif row.get("target") and not prev.get("target"):
                by_src[key] = row
            elif len(row["plan_file"]) > len(prev["plan_file"]):
                by_src[key] = row

    for key, prev in existing.items():
        row = by_src.get(key)
        if row:
            row["rust_status"] = prev["rust_status"]
            if prev.get("apply_note"):
                row["apply_note"] = prev["apply_note"]
        else:
            by_src[key] = prev

    items = sorted(by_src.values(), key=lambda x: (x["rust_status"] == "pending", x["verdict"], x["source_py"]))
    QUEUE.parent.mkdir(parents=True, exist_ok=True)
    QUEUE.write_text(json.dumps(items, indent=2) + "\n", encoding="utf-8")
    print(f"Wrote {len(items)} entries → {QUEUE}")
    for v in ("PORT_TO_PIPELINES", "MERGE_INTO_LIVE", "ARCHIVE_STUB"):
        n = sum(1 for i in items if i["verdict"] == v)
        print(f"  {v}: {n}")
    print(f"  rust ported: {sum(1 for i in items if i['rust_status'] == 'ported')}")
    print(f"  merged: {sum(1 for i in items if i['rust_status'] == 'merged')}")
    print(f"  archived: {sum(1 for i in items if i['rust_status'] == 'archived')}")
    print(f"  pending: {sum(1 for i in items if i['rust_status'] == 'pending')}")


if __name__ == "__main__":
    main()
