#!/usr/bin/env python3
/// Apply Rust from split_rust plans. Does NOT auto-wire mod.rs — review + cargo test first.
from __future__ import annotations

import json
import re
import subprocess
from pathlib import Path

REPO = Path("/codebase/repos/wreckhunter2000-1")
PLANS = REPO / "integrate_out" / "split_rust"
INFER = REPO / "cesarops-inference"
INTEGRATE = INFER / "src" / "integrate"
MOD = INTEGRATE / "mod.rs"
QUEUE = REPO / "integrate" / "PYTHON_TO_RUST_QUEUE.json"
APPLIED = REPO / "integrate" / "RUST_APPLIED.json"


def extract_rust(md_text: str) -> str | None:
    m = re.search(r"```rust\s*\n(.*?)```", md_text, re.S)
    if not m:
        return None
    return m.group(1).strip() + "\n"


def rust_path_from_plan(md_text: str, fallback: str) -> Path:
    m = re.search(r"^## Rust path\s*\n(.+)", md_text, re.M)
    raw = (m.group(1).strip() if m else fallback).strip("`").strip()
    name = Path(raw).name
    return INTEGRATE / name


def mod_name(rs_path: Path) -> str:
    return rs_path.stem.replace("-", "_")


def ensure_mod_decl(name: str) -> None:
    text = MOD.read_text(encoding="utf-8")
    decl = f"pub mod {name};"
    if decl in text:
        return
    anchor = "pub mod cesarops_orchestrator;"
    if anchor in text:
        text = text.replace(anchor, f"{anchor}\npub mod {name};")
    else:
        text = text.rstrip() + f"\n{decl}\n"
    MOD.write_text(text, encoding="utf-8")


def plan_quality(path: Path) -> bool:
    t = path.read_text(encoding="utf-8", errors="replace")
    return len(t) > 400 and "## Verdict" in t and "```rust" in t


def main() -> None:
    queue = {e["source_py"]: e for e in json.loads(QUEUE.read_text())}
    applied: dict[str, str] = {}
    if APPLIED.is_file():
        applied = json.loads(APPLIED.read_text())

    written = []
    for md in sorted(PLANS.rglob("rust__*.md")):
        if not plan_quality(md):
            continue
        base = md.stem.replace("rust__", "")
        src_py = f"{base}.py"
        if src_py in applied:
            continue
        text = md.read_text(encoding="utf-8", errors="replace")
        rust_src = extract_rust(text)
        if not rust_src:
            continue
        fallback = queue.get(src_py, {}).get("rust_path", f"cesarops-inference/src/integrate/{base}.rs")
        rs_path = rust_path_from_plan(text, fallback)
        rs_path.parent.mkdir(parents=True, exist_ok=True)
        rs_path.write_text(rust_src, encoding="utf-8")
        # mod.rs wiring is manual — plans often use non-existent crates
        applied[src_py] = str(rs_path.relative_to(REPO))
        written.append(str(rs_path.relative_to(REPO)))

    APPLIED.write_text(json.dumps(applied, indent=2) + "\n", encoding="utf-8")
    print(f"applied {len(written)} modules")
    for w in written:
        print(f"  {w}")

    if written:
        proc = subprocess.run(
            ["cargo", "test", "--lib", "integrate::"],
            cwd=INFER,
            capture_output=True,
            text=True,
        )
        print(proc.stdout[-2000:] if len(proc.stdout) > 2000 else proc.stdout)
        if proc.returncode != 0:
            print(proc.stderr[-1500:] if len(proc.stderr) > 1500 else proc.stderr)
            print(f"cargo test failed ({proc.returncode}) — fix modules before re-apply")


if __name__ == "__main__":
    main()
