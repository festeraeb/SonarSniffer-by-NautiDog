#!/usr/bin/env python3
"""
Blueprint audit + dispatch prep.

Creates:
1) file_inventory.csv          - one row per code file
2) file_inventory.jsonl        - machine-friendly rows
3) overlap_summary.json        - overlap/coverage by blueprint module
4) llm_dispatch_batches.json   - chunks ready for R1/other LLM assignment

Goal: compare every program file against the 5-module blueprint and
prepare a distributed review queue.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import json
import os
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Dict, Iterable, List, Tuple

DEFAULT_ROOTS = [
    "scripts",
    "cesarops-inference/src",
    "cesarops-forge-v2/src",
    "cesarops-mcp-worker/src",
    "sonarsniffer/src",
    "wrecks_api",
]

SKIP_DIR_NAMES = {
    ".git",
    "target",
    "backup",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".cargo-docker",
}

CODE_EXTS = {".py", ".rs", ".sh", ".toml"}

BLUEPRINT_KEYWORDS: Dict[str, List[str]] = {
    "geometry_context": [
        "wellhead",
        "bsee",
        "noaa",
        "buffer",
        "shapefile",
        "hough",
        "circle",
        "line",
        "geometry",
        "geojson",
    ],
    "spectral_plume_clarity": [
        "sentinel",
        "landsat",
        "spm",
        "ndvi",
        "mussel",
        "turbidity",
        "spectral",
        "swir",
        "nir",
        "gdal",
        "ndarray",
    ],
    "sar_ripple_glint": [
        "sar",
        "asf",
        "ripple",
        "glint",
        "fft",
        "rustfft",
        "backscatter",
    ],
    "thermal_sync": [
        "thermal",
        "sst",
        "modis",
        "landsat",
        "tirs",
        "z-score",
        "anomaly",
    ],
    "altimetry_displacement": [
        "icesat",
        "atl03",
        "atl08",
        "hdf5",
        "photon",
        "kdtree",
        "swot",
        "altimetry",
    ],
}


@dataclass
class FileAudit:
    path: str
    language: str
    bytes: int
    lines: int
    sha1: str
    modules: List[str]
    module_score: int
    primary_module: str
    overlaps: int
    guessed_role: str
    guess_confidence: str
    guess_reason: str
    notes: str

ROLE_KEYWORDS: Dict[str, List[str]] = {
    "drift_analysis": [
        "drift",
        "current",
        "trajectory",
        "advection",
        "velocity",
        "flow model",
        "particle",
    ],
    "sonar_analysis": [
        "sonar",
        "waterfall",
        "side scan",
        "sidescan",
        "slant range",
        "echogram",
        "beam",
        "backscatter",
    ],
    "pdf_breaker": [
        "pdf",
        "ocr",
        "extract text",
        "report export",
        "document parse",
        "lopdf",
        "pdf_extract",
    ],
    "bag_unmasker": [
        ".bag",
        "bathymetry attributed grid",
        "bag",
        "unmask",
        "mask",
        "gridded depth",
        "grid unmask",
    ],
    "orchestration_dispatch": [
        "dispatch",
        "orchestr",
        "scheduler",
        "queue",
        "batch",
        "lane",
        "task",
        "mission",
    ],
    "mcp_tooling": ["mcp", "tool", "capabilities", "rpc", "server", "delegate"],
    "llm_inference_serving": [
        "llm",
        "inference",
        "model",
        "token",
        "generate",
        "kobold",
        "prompt",
    ],
    "web_ui_api": ["axum", "html", "dashboard", "api", "route", "stream", "webhook"],
    "data_ingest_etl": ["download", "parse", "extract", "ingest", "inventory", "index"],
    "devops_runtime": ["systemd", "docker", "service", "deploy", "bootstrap", "setup"],
    "evaluation_testing": ["test", "bench", "validate", "verify", "audit", "check"],
    "docs_config": ["readme", "docs", "spec", "toml", "yaml", "json", "env"],
}


def walk_code_files(base: Path, roots: List[str]) -> Iterable[Path]:
    for rel in roots:
        root = base / rel
        if not root.exists():
            continue
        for cur, dirnames, filenames in os.walk(root):
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIR_NAMES]
            cur_p = Path(cur)
            for name in filenames:
                p = cur_p / name
                if p.suffix.lower() in CODE_EXTS:
                    yield p


def file_language(path: Path) -> str:
    ext = path.suffix.lower()
    return {
        ".py": "python",
        ".rs": "rust",
        ".sh": "shell",
        ".toml": "toml",
    }.get(ext, "other")


def score_modules(text: str) -> Tuple[List[str], int, str]:
    low = text.lower()
    scored: Dict[str, int] = {}
    for module, words in BLUEPRINT_KEYWORDS.items():
        score = 0
        for w in words:
            if w in low:
                score += 1
        if score > 0:
            scored[module] = score
    if not scored:
        return [], 0, "unclassified"
    modules = sorted(scored.keys(), key=lambda m: scored[m], reverse=True)
    return modules, sum(scored.values()), modules[0]


def guess_role(path: str, text: str, language: str) -> Tuple[str, str, str]:
    low = text.lower()
    path_l = path.lower()
    scores: Dict[str, int] = {}
    for role, words in ROLE_KEYWORDS.items():
        score = 0
        for w in words:
            if w in low:
                score += 1
            if w in path_l:
                score += 1
        if score > 0:
            scores[role] = score

    if language == "toml":
        scores["docs_config"] = scores.get("docs_config", 0) + 2
    if path_l.endswith(".sh"):
        scores["devops_runtime"] = scores.get("devops_runtime", 0) + 2

    if not scores:
        return "unknown_misc", "low", "No strong keyword/path signals matched."
    ordered = sorted(scores.items(), key=lambda kv: kv[1], reverse=True)
    top_role, top_score = ordered[0]
    second_score = ordered[1][1] if len(ordered) > 1 else 0
    if top_score >= 8 or top_score - second_score >= 4:
        conf = "high"
    elif top_score >= 4:
        conf = "medium"
    else:
        conf = "low"
    return top_role, conf, f"role_score={top_score}, next_best={second_score}"


def audit_file(base: Path, path: Path) -> FileAudit:
    try:
        raw = path.read_bytes()
        text = raw.decode("utf-8", errors="ignore")
    except Exception:
        raw = b""
        text = ""
    language = file_language(path)
    modules, score, primary = score_modules(text)
    rel = str(path.relative_to(base))
    role, conf, reason = guess_role(rel, text, language)
    return FileAudit(
        path=rel,
        language=language,
        bytes=len(raw),
        lines=text.count("\n") + (1 if text else 0),
        sha1=hashlib.sha1(raw).hexdigest() if raw else "",
        modules=modules,
        module_score=score,
        primary_module=primary,
        overlaps=max(0, len(modules) - 1),
        guessed_role=role,
        guess_confidence=conf,
        guess_reason=reason,
        notes="",
    )


def write_outputs(out_dir: Path, audits: List[FileAudit], batch_size: int) -> None:
    out_dir.mkdir(parents=True, exist_ok=True)

    csv_path = out_dir / "file_inventory.csv"
    with csv_path.open("w", newline="", encoding="utf-8") as f:
        w = csv.writer(f)
        w.writerow(
            [
                "path",
                "language",
                "bytes",
                "lines",
                "sha1",
                "primary_module",
                "modules",
                "module_score",
                "overlaps",
                "guessed_role",
                "guess_confidence",
                "guess_reason",
                "notes",
            ]
        )
        for a in audits:
            w.writerow(
                [
                    a.path,
                    a.language,
                    a.bytes,
                    a.lines,
                    a.sha1,
                    a.primary_module,
                    ",".join(a.modules),
                    a.module_score,
                    a.overlaps,
                    a.guessed_role,
                    a.guess_confidence,
                    a.guess_reason,
                    a.notes,
                ]
            )

    jsonl_path = out_dir / "file_inventory.jsonl"
    with jsonl_path.open("w", encoding="utf-8") as f:
        for a in audits:
            f.write(json.dumps(asdict(a), ensure_ascii=True) + "\n")

    module_counts: Dict[str, int] = {k: 0 for k in BLUEPRINT_KEYWORDS}
    overlap_files = 0
    for a in audits:
        if a.primary_module in module_counts:
            module_counts[a.primary_module] += 1
        if a.overlaps > 0:
            overlap_files += 1
    summary = {
        "total_files": len(audits),
        "module_primary_counts": module_counts,
        "overlap_files": overlap_files,
        "unclassified": sum(1 for a in audits if a.primary_module == "unclassified"),
        "guessed_role_counts": {},
    }
    role_counts: Dict[str, int] = {}
    for a in audits:
        role_counts[a.guessed_role] = role_counts.get(a.guessed_role, 0) + 1
    summary["guessed_role_counts"] = dict(sorted(role_counts.items(), key=lambda kv: kv[1], reverse=True))
    (out_dir / "overlap_summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")

    sorted_audits = sorted(audits, key=lambda a: (a.primary_module, -a.module_score, a.path))
    batches = []
    for i in range(0, len(sorted_audits), batch_size):
        chunk = sorted_audits[i : i + batch_size]
        batches.append(
            {
                "batch_id": f"batch-{(i // batch_size) + 1:03d}",
                "size": len(chunk),
                "paths": [a.path for a in chunk],
                "instruction": (
                    "For each file: summarize purpose, map to blueprint module(s), "
                    "note overlap/redundancy, and rate usefulness 1-5."
                ),
            }
        )
    (out_dir / "llm_dispatch_batches.json").write_text(json.dumps(batches, indent=2), encoding="utf-8")


def main() -> int:
    parser = argparse.ArgumentParser(description="Blueprint audit + LLM dispatch prep")
    parser.add_argument("--repo", default="/codebase/repos/wreckhunter2000-1")
    parser.add_argument("--roots", default=",".join(DEFAULT_ROOTS))
    parser.add_argument("--out-dir", default="/codebase/repos/wreckhunter2000-1/reports/blueprint_audit")
    parser.add_argument("--batch-size", type=int, default=120)
    parser.add_argument("--include-backup", action="store_true")
    args = parser.parse_args()

    base = Path(args.repo)
    roots = [r.strip() for r in args.roots.split(",") if r.strip()]
    if args.include_backup:
        roots.extend(
            [
                "backup/deploy/tools",
                "backup/src",
                "backup/wreckhunter2000-1",
            ]
        )
    audits = [audit_file(base, p) for p in walk_code_files(base, roots)]
    write_outputs(Path(args.out_dir), audits, args.batch_size)
    print(
        json.dumps(
            {
                "audited_files": len(audits),
                "out_dir": args.out_dir,
                "roots": roots,
            },
            indent=2,
        )
    )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

