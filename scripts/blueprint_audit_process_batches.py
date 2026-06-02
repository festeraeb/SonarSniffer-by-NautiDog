#!/usr/bin/env python3
"""
Process blueprint audit dispatch batches into concrete result artifacts.

This is a deterministic fallback processor for when LLM dispatch is unavailable.
It transforms file inventory rows into per-batch analysis outputs so downstream
tools can continue with stable JSON artifacts.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Dict, List


def usefulness_score(module_score: int, overlaps: int, confidence: str) -> int:
    score = 2
    if module_score >= 6:
        score += 2
    elif module_score >= 3:
        score += 1
    if overlaps > 0:
        score += 1
    if confidence == "high":
        score += 1
    return max(1, min(5, score))


def summarize_purpose(row: Dict[str, object]) -> str:
    primary = row.get("primary_module", "unclassified")
    role = row.get("guessed_role", "unknown_misc")
    lang = row.get("language", "other")
    if primary and primary != "unclassified":
        return f"{lang} source aligned primarily to {primary} with role hint {role}."
    return f"{lang} source not strongly mapped to blueprint modules; role hint {role}."


def load_jsonl(path: Path) -> Dict[str, Dict[str, object]]:
    out: Dict[str, Dict[str, object]] = {}
    with path.open("r", encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line:
                continue
            row = json.loads(line)
            out[row["path"]] = row
    return out


def process_batches(
    inventory_map: Dict[str, Dict[str, object]],
    batches: List[Dict[str, object]],
    out_dir: Path,
) -> Dict[str, object]:
    out_dir.mkdir(parents=True, exist_ok=True)
    manifest_batches: List[Dict[str, object]] = []
    total_files = 0
    missing_files: List[str] = []

    for batch in batches:
        batch_id = str(batch["batch_id"])
        paths = list(batch.get("paths", []))
        rows: List[Dict[str, object]] = []
        for p in paths:
            rec = inventory_map.get(p)
            if not rec:
                missing_files.append(p)
                continue
            module_score = int(rec.get("module_score", 0))
            overlaps = int(rec.get("overlaps", 0))
            confidence = str(rec.get("guess_confidence", "low"))
            modules = rec.get("modules", [])
            rows.append(
                {
                    "path": p,
                    "purpose_summary": summarize_purpose(rec),
                    "primary_module": rec.get("primary_module", "unclassified"),
                    "modules": modules,
                    "overlap_redundancy": (
                        "overlap_detected" if overlaps > 0 else "no_blueprint_overlap"
                    ),
                    "usefulness_1_to_5": usefulness_score(module_score, overlaps, confidence),
                    "guessed_role": rec.get("guessed_role", "unknown_misc"),
                    "guess_confidence": confidence,
                    "guess_reason": rec.get("guess_reason", ""),
                    "module_score": module_score,
                    "lines": int(rec.get("lines", 0)),
                    "language": rec.get("language", "other"),
                }
            )
        rows.sort(key=lambda x: (-x["usefulness_1_to_5"], x["path"]))
        result = {
            "batch_id": batch_id,
            "requested_size": int(batch.get("size", len(paths))),
            "processed_size": len(rows),
            "instruction": batch.get("instruction", ""),
            "results": rows,
        }
        out_path = out_dir / f"{batch_id}.json"
        out_path.write_text(json.dumps(result, indent=2), encoding="utf-8")
        manifest_batches.append(
            {
                "batch_id": batch_id,
                "out_file": str(out_path),
                "processed_size": len(rows),
                "requested_size": int(batch.get("size", len(paths))),
            }
        )
        total_files += len(rows)

    manifest = {
        "status": "ok",
        "total_batches": len(batches),
        "total_processed_files": total_files,
        "missing_files_count": len(missing_files),
        "missing_files": missing_files[:200],
        "batches": manifest_batches,
    }
    (out_dir / "manifest.json").write_text(json.dumps(manifest, indent=2), encoding="utf-8")
    return manifest


def main() -> int:
    parser = argparse.ArgumentParser(description="Process blueprint audit batches into artifacts")
    parser.add_argument(
        "--inventory-jsonl",
        default="/codebase/repos/wreckhunter2000-1/reports/blueprint_audit/file_inventory.jsonl",
    )
    parser.add_argument(
        "--batches-json",
        default="/codebase/repos/wreckhunter2000-1/reports/blueprint_audit/llm_dispatch_batches.json",
    )
    parser.add_argument(
        "--out-dir",
        default="/codebase/repos/wreckhunter2000-1/reports/blueprint_audit/llm_results",
    )
    args = parser.parse_args()

    inventory_path = Path(args.inventory_jsonl)
    batches_path = Path(args.batches_json)
    out_dir = Path(args.out_dir)

    inventory = load_jsonl(inventory_path)
    batches = json.loads(batches_path.read_text(encoding="utf-8"))
    manifest = process_batches(inventory, batches, out_dir)
    print(json.dumps(manifest, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
