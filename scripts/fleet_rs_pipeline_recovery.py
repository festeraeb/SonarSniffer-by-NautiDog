#!/usr/bin/env python3
"""
Deep RS pipeline recovery analysis — satellite, magnetics, BAG/sonar, fusion, etc.

Compares live repo vs laptopdump programming tree:
  - classifies maturity (stub / partial / complete)
  - tags RS domain(s)
  - finds dump copies that are more complete than repo stubs
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import time
from collections import defaultdict
from difflib import SequenceMatcher
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
DUMP_ROOTS = [
    "/mnt/t440/data/laptopdump/programming",
    "/data/laptopdump/programming",
]
REPO_ROOT = REPO
SKIP_DIR = {
    ".git", "node_modules", ".venv", "venv", "__pycache__", ".cache",
    "target", ".cargo-docker", "downloads", "outputs", "Documents",
    ".stfolder", "gstreamer", "site-packages", "Lib", "lib/python",
    "backup", "integrate", "integrate_out", ".cargo-docker",
}
REPO_SCAN_SUBDIRS = (
    "scripts", "pipelines", "missions", "cesarops-detection", "cesarops-forge-v2",
    "sonarsniffer", "config", "infra",
)
CODE_EXTS = {".py", ".sh", ".rs", ".ps1"}

DOMAIN_RULES: list[tuple[str, re.Pattern[str]]] = [
    ("satellite", re.compile(
        r"satellite|sentinel|landsat|hls|stac|earthdata|planet|modis|viirs|"
        r"download_erie|erie_remote|geotiff|rasterio|gdal|remote.?sensing",
        re.I,
    )),
    ("magnetics", re.compile(
        r"magnet|aeromag|curvelet|mag_pipeline|mag_data|geomag|rtp|tmi|"
        r"magnetic|usgs_mag|grid.?mag",
        re.I,
    )),
    ("bag_sonar", re.compile(
        r"bag.?file|\.bag|mbag|sonar|sidescan|sniffer|hydrographic|multibeam",
        re.I,
    )),
    ("detection_fusion", re.compile(
        r"detect|fusion|wreck|tile|glint|inference|yolo|segment",
        re.I,
    )),
    ("orchestration", re.compile(
        r"orchestrat|pipeline|worker.?bee|mission|prep_post|resume_satellite",
        re.I,
    )),
]

STUB_RE = re.compile(
    r"\b(stub|notimplemented|todo\b|fixme\b|wip\b|placeholder)\b|"
    r"raise\s+NotImplementedError|^\s*pass\s*$",
    re.I | re.M,
)


def log(msg: str) -> None:
    print(f"[rs-recovery] {msg}", flush=True)


def should_skip(root: str) -> bool:
    return any(p in Path(root).parts for p in SKIP_DIR)


def read_file(path: Path, limit: int = 80000) -> str:
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


def score_maturity(content: str, path: Path) -> tuple[str, dict[str, Any]]:
    lines = [ln for ln in content.splitlines() if ln.strip() and not ln.strip().startswith("#")]
    n = len(content.splitlines())
    loc = len(lines)
    stub_hits = len(STUB_RE.findall(content[:6000]))
    defs = re.findall(r"^\s*def\s+(\w+)", content, re.M)
    pass_defs = len(re.findall(r"^\s*def\s+\w+[^:]*:\s*pass\s*$", content, re.M))
    has_main = bool(re.search(r'if\s+__name__\s*==\s*["\']__main__', content))
    has_run = bool(re.search(r"def\s+(run|main|pipeline|execute|orchestrate)\w*\s*\(", content))
    imports = re.findall(r"^(?:import|from)\s+[\w.]+", content, re.M)[:30]
    score = loc + len(defs) * 8 + (20 if has_main else 0) + (15 if has_run else 0) - stub_hits * 5 - pass_defs * 10

    if loc < 8 or (n < 15 and stub_hits >= 2):
        mat = "stub"
    elif stub_hits >= 3 or (pass_defs == len(defs) and defs and loc < 60):
        mat = "stub"
    elif stub_hits >= 1 or pass_defs > 0:
        mat = "partial" if score >= 45 else "stub"
    elif score >= 55 or (has_run and loc > 40):
        mat = "complete"
    elif loc > 25:
        mat = "partial"
    else:
        mat = "stub"

    return mat, {
        "loc": loc,
        "lines": n,
        "defs": len(defs),
        "pass_defs": pass_defs,
        "stub_hits": stub_hits,
        "score": score,
        "imports": imports[:12],
        "has_main": has_main,
        "has_run": has_run,
    }


def infer_domains(path: Path, content: str) -> list[str]:
    blob = f"{path} {content[:4000]}"
    found = [name for name, pat in DOMAIN_RULES if pat.search(blob)]
    return found or ["general_rs"]


def infer_purpose(content: str, path: Path) -> str:
    m = re.search(r'^\s*("""|\'\'\')(.*?)\1', content, re.S)
    if m and len(m.group(2).strip()) > 10:
        return re.sub(r"\s+", " ", m.group(2).strip())[:500]
    for line in content.splitlines()[:25]:
        if line.strip().startswith("#") and len(line.strip()) > 14:
            return line.strip("# ").strip()[:500]
    return path.stem.replace("_", " ")


def is_rs_artifact(path: Path, content: str, domains: list[str]) -> bool:
    if domains != ["general_rs"]:
        return True
    rel = str(path).lower()
    if any(x in rel for x in (
        "pipeline", "orchestrat", "scanner", "processor", "download",
        "erie", "wreck", "mag_", "/mag/", "bag_", "sonar", "satellite",
    )):
        return True
    if path.suffix.lower() in CODE_EXTS and any(
        k in content.lower()[:3000]
        for k in ("gdal", "rasterio", "geotiff", "sentinel", "magnetic", "bag")
    ):
        return True
    return False


def _repo_walk_roots(repo: Path) -> list[Path]:
    out: list[Path] = []
    for sub in REPO_SCAN_SUBDIRS:
        p = repo / sub
        if p.is_dir():
            out.append(p)
    if not out:
        out.append(repo)
    return out


def walk_code(roots: list[Path], label: str) -> list[dict[str, Any]]:
    rows: list[dict[str, Any]] = []
    for root in roots:
        if not root.is_dir():
            continue
        if "site-packages" in root.parts or ".venv" in root.parts:
            continue
        for dirpath, dirnames, filenames in os.walk(root):
            if should_skip(dirpath):
                dirnames[:] = []
                continue
            dirnames[:] = [d for d in dirnames if d not in SKIP_DIR]
            for fn in filenames:
                p = Path(dirpath) / fn
                if p.suffix.lower() not in CODE_EXTS:
                    continue
                try:
                    st = p.stat()
                except OSError:
                    continue
                if st.st_size > 3_000_000:
                    continue
                content = read_file(p)
                if not content.strip():
                    continue
                domains = infer_domains(p, content)
                if not is_rs_artifact(p, content, domains):
                    continue
                maturity, metrics = score_maturity(content, p)
                digest = sha256_file(p) if st.st_size < 1_500_000 else None
                rows.append({
                    "path": str(p),
                    "origin": label,
                    "name": p.name,
                    "stem": p.stem.lower(),
                    "domains": domains,
                    "maturity": maturity,
                    "purpose": infer_purpose(content, p),
                    "metrics": metrics,
                    "sha256": digest,
                    "bytes": st.st_size,
                })
    return rows


def purpose_similarity(a: str, b: str) -> float:
    return SequenceMatcher(None, a.lower()[:200], b.lower()[:200]).ratio()


def find_recovery_pairs(repo_rows: list[dict], dump_rows: list[dict]) -> list[dict[str, Any]]:
    """Repo stub/partial → better complete/partial in dump."""
    repo_weak = [r for r in repo_rows if r["maturity"] in ("stub", "partial")]
    dump_strong = [r for r in dump_rows if r["maturity"] in ("complete", "partial")]

    by_name_dump: dict[str, list[dict]] = defaultdict(list)
    by_stem_dump: dict[str, list[dict]] = defaultdict(list)
    for d in dump_strong:
        by_name_dump[d["name"]].append(d)
        by_stem_dump[d["stem"]].append(d)

    pairs: list[dict[str, Any]] = []
    seen: set[str] = set()

    for r in repo_weak:
        candidates: list[dict] = []
        # exact basename
        for d in by_name_dump.get(r["name"], []):
            if d.get("sha256") and d["sha256"] == r.get("sha256"):
                continue
            if d["metrics"]["score"] > r["metrics"]["score"] + 15:
                candidates.append(d)
        # stem match (e.g. mag_pipeline_stage vs mag_pipeline_stage_v2)
        if not candidates:
            for d in by_stem_dump.get(r["stem"], []):
                if d["metrics"]["score"] > r["metrics"]["score"] + 20:
                    candidates.append(d)
        # domain + purpose similarity (same stem prefix, not generic test_* noise)
        if not candidates and not r["stem"].startswith("test_"):
            for d in dump_strong:
                if not set(r["domains"]) & set(d["domains"]):
                    continue
                if purpose_similarity(r["purpose"], d["purpose"]) < 0.62:
                    continue
                if d["stem"] == r["stem"]:
                    continue
                if ".venv" in d["path"] or "site-packages" in d["path"]:
                    continue
                if d["metrics"]["score"] <= r["metrics"]["score"] + 25:
                    continue
                candidates.append(d)

        candidates.sort(key=lambda x: -x["metrics"]["score"])
        for d in candidates[:3]:
            key = f"{r['path']}|{d['path']}"
            if key in seen:
                continue
            seen.add(key)
            pairs.append({
                "repo_path": r["path"],
                "repo_maturity": r["maturity"],
                "repo_score": r["metrics"]["score"],
                "repo_purpose": r["purpose"][:300],
                "dump_path": d["path"],
                "dump_maturity": d["maturity"],
                "dump_score": d["metrics"]["score"],
                "dump_purpose": d["purpose"][:300],
                "domains": list(set(r["domains"]) & set(d["domains"])),
                "match": "basename" if r["name"] == d["name"] else (
                    "stem" if r["stem"] == d["stem"] else "domain_purpose"
                ),
                "recommendation": "replace_repo_with_dump" if d["maturity"] == "complete" else "review_merge",
            })
            break
    pairs.sort(key=lambda x: (-x["dump_score"] + x["repo_score"]))
    return pairs


def cluster_duplicates(rows: list[dict]) -> list[dict[str, Any]]:
    by_sha: dict[str, list[str]] = defaultdict(list)
    by_name: dict[str, list[dict]] = defaultdict(list)
    for r in rows:
        if r.get("sha256"):
            by_sha[r["sha256"]].append(r["path"])
        by_name[r["name"]].append(r)

    exact = [{"sha256": h, "paths": ps, "count": len(ps)} for h, ps in by_sha.items() if len(ps) > 1]
    name_groups = []
    for name, group in by_name.items():
        if len(group) < 2:
            continue
        mats = {g["maturity"] for g in group}
        origins = {g["origin"] for g in group}
        hashes = {g.get("sha256") for g in group}
        if len(hashes) > 1:
            name_groups.append({
                "basename": name,
                "count": len(group),
                "maturities": sorted(mats),
                "origins": sorted(origins),
                "paths": [g["path"] for g in sorted(group, key=lambda x: -x["metrics"]["score"])[:8]],
                "best": max(group, key=lambda x: x["metrics"]["score"])["path"],
                "verdict": "different_implementation" if len(mats) > 1 else "duplicate_copy",
            })
    return exact, name_groups


def write_report(
    out_dir: Path,
    repo_rows: list[dict],
    dump_rows: list[dict],
    pairs: list[dict],
    exact: list,
    name_groups: list,
) -> None:
    all_rows = repo_rows + dump_rows
    by_dom: dict[str, dict[str, int]] = defaultdict(lambda: defaultdict(int))
    for r in all_rows:
        for d in r["domains"]:
            by_dom[d][r["maturity"]] += 1

    lines = [
        "# RS pipeline recovery — deep analysis",
        "",
        "Satellite, magnetics, BAG/sonar, detection/fusion scripts across **repo** vs **laptopdump**.",
        "",
        f"| source | artifacts | stub | partial | complete |",
        f"|--------|-----------|------|---------|----------|",
    ]
    for label, rows in (("repo", repo_rows), ("laptopdump", dump_rows)):
        mc = defaultdict(int)
        for r in rows:
            mc[r["maturity"]] += 1
        lines.append(
            f"| {label} | {len(rows)} | {mc['stub']} | {mc['partial']} | {mc['complete']} |"
        )

    lines += ["", "## By RS domain (maturity counts)", ""]
    for dom in sorted(by_dom.keys()):
        c = by_dom[dom]
        lines.append(
            f"- **{dom}**: stub={c.get('stub',0)} partial={c.get('partial',0)} "
            f"complete={c.get('complete',0)}"
        )

    lines += [
        "",
        f"## Recovery: repo stub/partial → better copy in dump ({len(pairs)} matches)",
        "",
        "These are candidates where the dump may have the implementation you thought was lost.",
        "",
    ]
    for p in pairs[:60]:
        lines.append(f"### `{Path(p['repo_path']).name}` ({', '.join(p['domains'])})")
        lines.append(f"- **repo** `{p['repo_path']}` — {p['repo_maturity']} (score {p['repo_score']})")
        lines.append(f"  - {p['repo_purpose'][:120]}…" if len(p["repo_purpose"]) > 120 else f"  - {p['repo_purpose']}")
        lines.append(f"- **dump** `{p['dump_path']}` — {p['dump_maturity']} (score {p['dump_score']}) [{p['match']}]")
        lines.append(f"  - {p['dump_purpose'][:120]}…" if len(p["dump_purpose"]) > 120 else f"  - {p['dump_purpose']}")
        lines.append(f"- **→ {p['recommendation']}**")
        lines.append("")

    lines += ["", "## Same filename, multiple implementations", ""]
    for g in name_groups[:40]:
        if "complete" in g["maturities"] and "stub" in g["maturities"]:
            lines.append(f"- `{g['basename']}` — best: `{g['best']}`")
            for p in g["paths"][:4]:
                lines.append(f"  - `{p}`")

    lines += ["", "## Exact duplicates (SHA)", "", f"{len(exact)} groups", ""]
    for e in exact[:15]:
        lines.append(f"- {e['count']}× " + ", ".join(f"`{p}`" for p in e["paths"][:3]))

    (out_dir / "rs_pipeline_recovery_report.md").write_text("\n".join(lines), encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out-dir", default="")
    args = ap.parse_args()

    out_dir = Path(args.out_dir) if args.out_dir else REPO / "var/fleet-catalog/rs_pipeline_recovery"
    out_dir.mkdir(parents=True, exist_ok=True)

    dump_roots = [Path(p) for p in DUMP_ROOTS if Path(p).is_dir()]
    t0 = time.time()
    log(f"scanning repo {REPO_ROOT} ({REPO_SCAN_SUBDIRS})")
    repo_rows = walk_code(_repo_walk_roots(REPO_ROOT), "repo")
    log(f"repo RS artifacts: {len(repo_rows)}")
    log(f"scanning dump {[str(p) for p in dump_roots]}")
    dump_rows = walk_code(dump_roots, "laptopdump")
    log(f"dump RS artifacts: {len(dump_rows)}")

    pairs = find_recovery_pairs(repo_rows, dump_rows)
    exact, name_groups = cluster_duplicates(repo_rows + dump_rows)

    with (out_dir / "repo_artifacts.jsonl").open("w", encoding="utf-8") as f:
        for r in repo_rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    with (out_dir / "dump_artifacts.jsonl").open("w", encoding="utf-8") as f:
        for r in dump_rows:
            f.write(json.dumps(r, ensure_ascii=False) + "\n")
    with (out_dir / "recovery_pairs.jsonl").open("w", encoding="utf-8") as f:
        for p in pairs:
            f.write(json.dumps(p, ensure_ascii=False) + "\n")

    summary = {
        "elapsed_s": round(time.time() - t0, 1),
        "repo_count": len(repo_rows),
        "dump_count": len(dump_rows),
        "recovery_pairs": len(pairs),
        "exact_duplicate_groups": len(exact),
        "same_name_multi_impl": len(name_groups),
        "repo_stubs": sum(1 for r in repo_rows if r["maturity"] == "stub"),
        "dump_complete": sum(1 for r in dump_rows if r["maturity"] == "complete"),
    }
    (out_dir / "summary.json").write_text(json.dumps(summary, indent=2), encoding="utf-8")
    write_report(out_dir, repo_rows, dump_rows, pairs, exact, name_groups)

    log(f"recovery pairs: {len(pairs)} | repo stubs: {summary['repo_stubs']} | dump complete: {summary['dump_complete']}")
    log(f"report -> {out_dir / 'rs_pipeline_recovery_report.md'}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
