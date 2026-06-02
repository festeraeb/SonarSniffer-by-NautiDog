#!/usr/bin/env python3
"""
Inventory pipelines & scripts from laptopdump + live repo.

Classifies each artifact: stub | partial | complete | config
Indexes unified pipeline profiles + mission specs
Flags exact duplicates vs same-name-different-implementation

No GDAL / GeoTIFF — code and pipeline configs only.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
from collections import defaultdict
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))

TEXT_EXTS = {".py", ".sh", ".bash", ".rs", ".js", ".ts", ".tsx", ".jsx", ".toml", ".yaml", ".yml"}
CONFIG_EXTS = {".json", ".nomad", ".hcl"}
SKIP_DIR = {
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "target",
    ".cargo-docker",
    "downloads",
    "outputs",
    "Documents",
    ".stfolder",
}
PIPELINE_NAME_RE = re.compile(r"pipeline|orchestrat|worker.?bee|prep_post|resume_satellite", re.I)
STUB_RE = re.compile(
    r"\b(stub|notimplemented|todo\b|fixme\b|wip\b|placeholder|coming soon)\b|"
    r"raise\s+NotImplementedError|^\s*pass\s*$",
    re.I | re.M,
)


def log(msg: str) -> None:
    print(f"[pipeline-inv] {msg}", flush=True)


def should_skip(root: str) -> bool:
    return any(part in Path(root).parts for part in SKIP_DIR)


def read_head(path: Path, limit: int = 12000) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore")[:limit]
    except OSError:
        return ""


def sha256_file(path: Path) -> str | None:
    try:
        h = hashlib.sha256()
        with path.open("rb") as f:
            for chunk in iter(lambda: f.read(65536), b""):
                h.update(chunk)
        return h.hexdigest()
    except OSError:
        return None


def normalize_for_sim(content: str) -> str:
    lines = []
    for line in content.splitlines():
        s = line.strip()
        if not s or s.startswith("#"):
            continue
        lines.append(re.sub(r"\s+", " ", s))
    return "\n".join(lines)


def classify_maturity(path: Path, content: str) -> str:
    if path.suffix.lower() in CONFIG_EXTS:
        if "pipeline" in content.lower() or "run_" in content:
            return "config"
        return "config"

    lines = content.splitlines()
    n = len(lines)
    lower = content.lower()
    stub_hits = len(STUB_RE.findall(content[:4000]))
    def_count = len(re.findall(r"^\s*def\s+\w+", content, re.M))
    pass_defs = len(re.findall(r"^\s*def\s+\w+[^:]*:\s*pass\s*$", content, re.M))
    has_main = bool(re.search(r'if\s+__name__\s*==\s*["\']__main__', content))
    has_cli = bool(re.search(r"argparse|click\.|typer\.", content))
    has_run_fn = bool(re.search(r"def\s+(run|main|pipeline|execute|orchestrate)\w*\s*\(", content))

    if n < 12 or (len(content.strip()) < 30):
        return "stub"
    if stub_hits >= 4 or (pass_defs == def_count and def_count > 0 and n < 50):
        return "stub"
    if "stub" in path.name.lower() or path.name.startswith("test_") and n < 80:
        if stub_hits >= 2:
            return "stub"
    if stub_hits >= 2 and not has_run_fn and n < 100:
        return "stub"
    if stub_hits >= 1 or pass_defs > 0 or "wip" in lower[:600]:
        if has_main and n > 120 and def_count >= 3:
            return "partial"
        if has_run_fn and n > 80:
            return "partial"
        return "stub" if stub_hits >= 2 else "partial"
    if has_main or has_cli or has_run_fn or (def_count >= 2 and n > 70):
        return "complete"
    if n < 45:
        return "stub"
    return "partial"


def infer_purpose(path: Path, content: str) -> str:
    m = re.search(r'^\s*("""|\'\'\')(.*?)\1', content, re.S)
    if m and len(m.group(2).strip()) > 8:
        return re.sub(r"\s+", " ", m.group(2).strip())[:400]
    for line in content.splitlines()[:20]:
        t = line.strip()
        if t.startswith("#") and len(t) > 12:
            return t.lstrip("# ").strip()[:400]
    if path.suffix == ".sh" and content.startswith("#!"):
        for line in content.splitlines()[1:8]:
            if line.strip().startswith("#"):
                return line.lstrip("# ").strip()[:400]
    return path.stem.replace("_", " ").replace("-", " ")


def is_pipeline_candidate(path: Path) -> bool:
    rel = str(path).lower()
    if PIPELINE_NAME_RE.search(rel) or PIPELINE_NAME_RE.search(path.name):
        return True
    if path.parent.name in ("pipelines", "pipeline", "scripts") and path.suffix in {".py", ".sh"}:
        return True
    return False


def walk_roots(roots: list[str]) -> list[Path]:
    out: list[Path] = []
    for root_s in roots:
        root = Path(root_s)
        if not root.is_dir():
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            if should_skip(dirpath):
                dirnames[:] = []
                continue
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIR]
            for fn in filenames:
                p = Path(dirpath) / fn
                ext = p.suffix.lower()
                if ext not in TEXT_EXTS and ext not in CONFIG_EXTS:
                    continue
                if ext in {".tif", ".tiff", ".nc", ".bag"}:
                    continue
                out.append(p)
    return out


def index_repo_pipelines(repo: Path) -> list[dict[str, Any]]:
    entries: list[dict[str, Any]] = []
    prof_path = repo / "config/unified_pipeline_profiles.json"
    if prof_path.is_file():
        data = json.loads(prof_path.read_text(encoding="utf-8"))
        for name, spec in (data.get("profiles") or {}).items():
            patch = spec.get("patch") or spec
            entries.append(
                {
                    "id": name,
                    "source": "unified_pipeline_profiles.json",
                    "type": "profile",
                    "pipeline_flags": patch.get("pipeline") or {},
                    "objective": patch.get("objective"),
                    "sensors": patch.get("sensors"),
                }
            )
    for mp in sorted((repo / "missions").glob("*.json")):
        try:
            m = json.loads(mp.read_text(encoding="utf-8"))
        except (json.JSONDecodeError, OSError):
            continue
        if "pipeline" in m or "pipeline_steps" in m:
            entries.append(
                {
                    "id": mp.stem,
                    "source": f"missions/{mp.name}",
                    "type": "mission",
                    "pipeline": m.get("pipeline"),
                    "pipeline_steps": m.get("pipeline_steps"),
                }
            )
    for rel in ("scripts", "pipelines"):
        base = repo / rel
        if not base.is_dir():
            continue
        for p in base.rglob("*"):
            if p.is_file() and is_pipeline_candidate(p) and p.suffix in TEXT_EXTS:
                entries.append(
                    {
                        "id": str(p.relative_to(repo)),
                        "source": "repo_tree",
                        "type": "script",
                        "path": str(p),
                    }
                )
    return entries


def analyze_file(path: Path, source: str) -> dict[str, Any] | None:
    if path.suffix.lower() not in TEXT_EXTS | CONFIG_EXTS:
        return None
    try:
        st = path.stat()
    except OSError:
        return None
    content = read_head(path, 16000)
    if not content and st.st_size > 0:
        return None
    maturity = classify_maturity(path, content)
    purpose = infer_purpose(path, content)
    digest = sha256_file(path) if st.st_size < 2_000_000 else None
    norm_hash = hashlib.sha256(normalize_for_sim(content).encode()).hexdigest()[:16]
    return {
        "path": str(path),
        "source": source,
        "name": path.name,
        "ext": path.suffix.lower(),
        "bytes": st.st_size,
        "maturity": maturity,
        "purpose": purpose,
        "pipeline_candidate": is_pipeline_candidate(path),
        "sha256": digest,
        "norm_hash": norm_hash,
    }


def build_duplicate_report(rows: list[dict[str, Any]]) -> dict[str, Any]:
    by_sha: dict[str, list[str]] = defaultdict(list)
    by_name: dict[str, list[dict[str, Any]]] = defaultdict(list)
    by_norm: dict[str, list[str]] = defaultdict(list)
    for r in rows:
        if r.get("sha256"):
            by_sha[r["sha256"]].append(r["path"])
        by_name[r["name"]].append(r)
        by_norm[r["norm_hash"]].append(r["path"])

    exact_dupes = [{ "sha256": h, "paths": ps} for h, ps in by_sha.items() if len(ps) > 1]
    same_name_diff: list[dict[str, Any]] = []
    for name, group in by_name.items():
        if len(group) < 2:
            continue
        hashes = {g.get("sha256") or g.get("norm_hash") for g in group}
        if len(hashes) > 1:
            same_name_diff.append(
                {
                    "basename": name,
                    "paths": [g["path"] for g in group],
                    "maturities": [g["maturity"] for g in group],
                    "purposes": list({g["purpose"][:120] for g in group})[:4],
                    "verdict": "different_function"
                    if len({g["purpose"][:80] for g in group}) > 1
                    else "likely_copy_variant",
                }
            )
    near_dupes = [{ "norm_hash": h, "paths": ps[:8], "count": len(ps)} for h, ps in by_norm.items() if len(ps) > 1]

    return {
        "exact_duplicate_groups": len(exact_dupes),
        "exact_duplicates": exact_dupes[:200],
        "same_name_different_impl": same_name_diff[:300],
        "near_duplicate_norm_groups": len(near_dupes),
        "near_duplicates_sample": near_dupes[:80],
    }


def write_report(out_dir: Path, rows: list[dict[str, Any]], pipelines: list[dict[str, Any]], dupes: dict[str, Any]) -> None:
    mat = defaultdict(int)
    pipe_rows = [r for r in rows if r.get("pipeline_candidate")]
    for r in rows:
        mat[r["maturity"]] += 1

    md = [
        "# Pipeline & script inventory",
        "",
        f"Artifacts scanned: **{len(rows)}** | pipeline candidates: **{len(pipe_rows)}**",
        "",
        "## Maturity (stub → complete)",
        "",
        "| maturity | count |",
        "|----------|-------|",
    ]
    for k in ("stub", "partial", "complete", "config"):
        md.append(f"| {k} | {mat.get(k, 0)} |")
    md += [
        "",
        "## Current pipeline index (repo + profiles)",
        "",
        f"Registered entries: **{len(pipelines)}**",
        "",
    ]
    for p in pipelines[:40]:
        md.append(f"- `{p.get('id')}` — {p.get('source')} ({p.get('type')})")
    if len(pipelines) > 40:
        md.append(f"- … {len(pipelines) - 40} more")

    md += ["", "## Same name, different implementation", ""]
    for d in dupes.get("same_name_different_impl", [])[:35]:
        md.append(f"### `{d['basename']}` — {d['verdict']}")
        for p in d["paths"][:5]:
            md.append(f"- `{p}`")
        md.append(f"- purposes: {d.get('purposes', [])}")
        md.append("")

    md += ["", "## Stubs (sample)", ""]
    stubs = [r for r in pipe_rows if r["maturity"] == "stub"][:40]
    for r in stubs:
        md.append(f"- `{r['path']}` — {r['purpose'][:100]}")

    md += ["", "## Complete pipeline scripts (sample)", ""]
    done = [r for r in pipe_rows if r["maturity"] == "complete"][:40]
    for r in done:
        md.append(f"- `{r['path']}` — {r['purpose'][:100]}")

    (out_dir / "report.md").write_text("\n".join(md) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--roots", nargs="*", default=[])
    ap.add_argument("--out-dir", default="")
    ap.add_argument("--max-files", type=int, default=0)
    args = ap.parse_args()

    default_roots = [
        "/mnt/t440/data/laptopdump/programming",
        "/data/laptopdump/programming",
        str(REPO),
    ]
    roots = args.roots or [r for r in default_roots if Path(r).is_dir()]
    out_dir = Path(args.out_dir) if args.out_dir else REPO / "var/fleet-catalog/pipeline_inventory"
    out_dir.mkdir(parents=True, exist_ok=True)

    t0 = time.time()
    paths = walk_roots(roots)
    log(f"scanning {len(paths)} text files under {len(roots)} roots")

    rows: list[dict[str, Any]] = []
    for i, p in enumerate(paths):
        if args.max_files and i >= args.max_files:
            break
        src = "laptopdump" if "laptopdump" in str(p) else "repo"
        row = analyze_file(p, src)
        if row:
            rows.append(row)
        if (i + 1) % 5000 == 0:
            log(f"  {i + 1}/{len(paths)}")

    pipelines = index_repo_pipelines(REPO)
    dupes = build_duplicate_report(rows)

    jsonl = out_dir / "artifacts.jsonl"
    with jsonl.open("w", encoding="utf-8") as f:
        for row in rows:
            f.write(json.dumps(row, ensure_ascii=False) + "\n")

    mc: dict[str, int] = defaultdict(int)
    for r in rows:
        mc[r["maturity"]] += 1
    summary = {
        "scanned_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "roots": roots,
        "artifact_count": len(rows),
        "pipeline_candidates": sum(1 for r in rows if r.get("pipeline_candidate")),
        "maturity": dict(mc),
        "elapsed_s": round(time.time() - t0, 1),
        "current_pipelines": pipelines,
        "duplicates": dupes,
    }

    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    (out_dir / "pipeline_index.json").write_text(
        json.dumps(pipelines, indent=2), encoding="utf-8"
    )
    write_report(out_dir, rows, pipelines, dupes)
    log(f"wrote {jsonl} ({len(rows)} rows) -> {out_dir}")
    log(f"maturity: {dict(mc)} | pipeline_candidates: {summary['pipeline_candidates']}")
    log(f"exact_dup_groups: {dupes['exact_duplicate_groups']} | same_name_diff: {len(dupes['same_name_different_impl'])}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
