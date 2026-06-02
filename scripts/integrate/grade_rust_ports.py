#!/usr/bin/env python3
"""Grade integrate/*.rs ports against PORT_TABLE expectations."""
from __future__ import annotations

import json
import re
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
INTEGRATE = REPO / "cesarops-inference" / "src" / "integrate"
OUT_QWEN = REPO / "integrate_out" / "qwen14b_2060"
OUT_RUST = REPO / "integrate_out" / "rust14b_1070"
REPORT = REPO / "integrate_out" / "port_grades.json"
SUMMARY_MD = REPO / "integrate_out" / "PORT_GRADES.md"

# Import PORT_TABLE from sibling script
import sys

sys.path.insert(0, str(Path(__file__).parent))
from dispatch_c2_rust_port import PORT_TABLE  # noqa: E402

ALIASES = {
    "generate_db_master_key.rs": "db_master_key.rs",
    "run_zero.rs": "run_zero_baseline.rs",
    "three_tile_offset_analysis.rs": "three_tile_offset.rs",
}


def resolve_rust_path(rust_rel: str) -> Path:
    return REPO / rust_rel


def forge_md_path(py_rel: str, gpu: str) -> Path:
    base = Path(py_rel).name.replace(".py", "")
    out = OUT_RUST if gpu == "rust" else OUT_QWEN
    return out / f"rust__{base}.md"


def score_file(path: Path, py_rel: str) -> dict:
    if not path.is_file():
        return {"grade": "F", "score": 0, "notes": ["missing .rs"]}

    text = path.read_text(encoding="utf-8", errors="replace")
    lines = len(text.splitlines())
    has_tests = "#[cfg(test)]" in text or "#[test]" in text
    has_serde = "serde" in text
    has_pub_fn = "pub fn" in text
    has_struct = "pub struct" in text
    has_const = "pub const" in text
    has_todo_only = lines < 30 and not has_pub_fn
    subprocess_heavy = "subprocess" in text or "std::process::Command" in text
    io_heavy = "std::fs::" in text or "sqlite" in text.lower() or "rusqlite" in text

    score = 40
    notes: list[str] = []

    if has_struct or has_pub_fn:
        score += 15
    if has_const:
        score += 5
    if has_serde:
        score += 10
    if has_tests:
        score += 20
    if lines >= 80:
        score += 10
    elif lines >= 40:
        score += 5
    if has_todo_only:
        score -= 25
        notes.append("thin stub")
    if subprocess_heavy or io_heavy:
        notes.append("I/O not wired (logic-only port)")

    # Heuristic: compare basename keywords from python
    py_name = Path(py_rel).stem
    keyword_hits = sum(1 for w in py_name.split("_") if w and w in text.lower())
    if keyword_hits >= 2:
        score += 5

    score = max(0, min(100, score))

    if score >= 85:
        grade = "A"
    elif score >= 70:
        grade = "B"
    elif score >= 50:
        grade = "C"
    else:
        grade = "D"

    return {
        "grade": grade,
        "score": score,
        "lines": lines,
        "has_tests": has_tests,
        "notes": notes,
    }


def grade_forge_md(md: Path) -> dict | None:
    if not md.is_file():
        return None
    t = md.read_text(encoding="utf-8", errors="replace")
    ok = len(t) > 400 and "## Verdict" in t and "```rust" in t
    verdict = re.search(r"## Verdict\s*\n(\S+)", t)
    return {
        "present": True,
        "valid": ok,
        "len": len(t),
        "verdict": verdict.group(1) if verdict else None,
    }


def assign_gpu(idx: int) -> str:
    return "rust" if idx % 3 == 2 else "qwen"


def main() -> None:
    rows = []
    dist = {"A": 0, "B": 0, "C": 0, "D": 0, "F": 0}

    for idx, (item_id, py_rel, rust_rel) in enumerate(PORT_TABLE):
        rust_name = Path(rust_rel).name
        path = resolve_rust_path(rust_rel)
        if not path.is_file() and rust_name in ALIASES:
            path = path.parent / ALIASES[rust_name]

        gpu = assign_gpu(idx)
        rs = score_file(path, py_rel)
        md = grade_forge_md(forge_md_path(py_rel, gpu))
        dist[rs["grade"]] = dist.get(rs["grade"], 0) + 1

        row = {
            "id": item_id,
            "python": py_rel,
            "rust": str(path.relative_to(REPO)),
            "gpu": gpu,
            **rs,
            "forge_md": md,
        }
        rows.append(row)

    rows.sort(key=lambda r: (r["grade"], -r["score"], r["id"]))

    payload = {
        "graded": len(rows),
        "distribution": dist,
        "avg_score": round(sum(r["score"] for r in rows) / max(len(rows), 1), 1),
        "tests_pass": "cargo test --lib integrate:: (run separately)",
        "rows": rows,
    }
    REPORT.parent.mkdir(parents=True, exist_ok=True)
    REPORT.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")

    lines = [
        "# Rust port grades (51-module queue)",
        "",
        f"**Graded:** {payload['graded']} | **Avg score:** {payload['avg_score']}/100",
        "",
        "| Grade | Count | Meaning |",
        "|-------|-------|---------|",
        f"| A | {dist['A']} | Logic + tests + solid API |",
        f"| B | {dist['B']} | Good integrate-layer port |",
        f"| C | {dist['C']} | Stub/constants; needs depth |",
        f"| D | {dist['D']} | Very thin |",
        f"| F | {dist['F']} | Missing |",
        "",
        "## By module",
        "",
        "| ID | Grade | Score | Lines | Tests | Rust | Forge MD | Notes |",
        "|----|-------|-------|-------|-------|------|----------|-------|",
    ]
    for r in rows:
        md_ok = ""
        if r.get("forge_md"):
            md_ok = "yes" if r["forge_md"].get("valid") else "partial"
        else:
            md_ok = "—"
        notes = "; ".join(r.get("notes") or []) or "—"
        lines.append(
            f"| {r['id']} | **{r['grade']}** | {r['score']} | {r['lines']} | "
            f"{'yes' if r['has_tests'] else 'no'} | `{Path(r['rust']).name}` | {md_ok} | {notes} |"
        )

    SUMMARY_MD.write_text("\n".join(lines) + "\n", encoding="utf-8")
    print(json.dumps({"report": str(REPORT), "summary": str(SUMMARY_MD), "distribution": dist, "avg": payload["avg_score"]}, indent=2))


if __name__ == "__main__":
    main()
