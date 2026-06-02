#!/usr/bin/env python3
"""
Collect laptop / dump files vs live:
  - integrate/     — not in live, promising (complete or substantive stub)
  - live_reference/ — same logical target as live but different bytes (for P100 diff + merge)
"""
from __future__ import annotations

import hashlib
import json
import re
import shutil
import tempfile
import zipfile
from dataclasses import dataclass, asdict
from pathlib import Path
from typing import Iterable

REPO = Path("/codebase/repos/wreckhunter2000-1")
LIVE_PIPELINES = Path("/codebase/projects/pipelines")
INTEGRATE = REPO / "integrate"
LIVE_REF = REPO / "live_reference"

STUB_RE = re.compile(
    r"\b(TODO|FIXME|NotImplementedError|placeholder|not yet implemented|coming soon)\b",
    re.I,
)

SOURCES: list[tuple[str, Path]] = [
    ("laptop-code-pipelines", Path("/codebase/repos/laptop-code/pipelines")),
    ("laptopdump-wreckhunter-build", Path("/data/laptopdump/programming/cesarops-wreckhunter build")),
    ("laptopdump-programming-root", Path("/data/laptopdump/programming")),
]

SNAPSHOT_ZIP = Path(
    "/data/laptopdump/programming/cesarops-core/Documents/CESAROPS_COMPLETE_SNAPSHOT.zip"
)


def sha256_file(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for _ in range(16384):
            b = f.read(65536)
            if not b:
                break
            h.update(b)
    return h.hexdigest()


def analyze_py(text: str) -> dict:
    lines = text.splitlines()
    n = len(lines)
    non_empty = sum(1 for ln in lines if ln.strip())
    stubs = len(STUB_RE.findall(text))
    empty_funcs = len(
        re.findall(r"def \w+\([^)]*\):\s*\n\s*(pass|\.\.\.)", text)
    )
    stub_score = stubs * 3 + empty_funcs * 5 + (15 if n < 25 else 0) + (10 if non_empty < 8 else 0)
    return {"lines": n, "non_empty": non_empty, "stub_score": stub_score}


def promising(a: dict) -> bool:
    """Complete or substantive stub worth human/P100 review."""
    if a["lines"] < 35:
        return False
    if a["stub_score"] >= 25:
        return False
    if a["lines"] >= 120 and a["stub_score"] < 15:
        return True
    if a["lines"] >= 50 and a["stub_score"] < 12:
        return True
    if a["lines"] >= 35 and a["stub_score"] < 8:
        return True
    return False


def live_pipeline_files() -> dict[str, Path]:
    out: dict[str, Path] = {}
    if not LIVE_PIPELINES.exists():
        return out
    for p in LIVE_PIPELINES.rglob("*.py"):
        if "__pycache__" in p.parts:
            continue
        rel = p.relative_to(LIVE_PIPELINES)
        key = str(Path("pipelines") / rel).replace("\\", "/")
        out[key] = p
    return out


def live_repo_root_py() -> dict[str, Path]:
    out: dict[str, Path] = {}
    for p in REPO.glob("*.py"):
        key = f"repo_root/{p.name}"
        out[key] = p
    return out


def classify_flat_name(name: str, live_pipe: dict, live_root: dict) -> str | None:
    """Map foo.py to pipelines/mag/foo.py if exists in exactly one subtree, else repo_root."""
    hits = []
    for sub in ("mag", "satellite", "bag"):
        k = f"pipelines/{sub}/{name}"
        if k in live_pipe:
            hits.append(k)
    rk = f"repo_root/{name}"
    if rk in live_root:
        hits.append(rk)
    if len(hits) == 1:
        return hits[0]
    if len(hits) > 1:
        # Prefer mag > satellite > bag if multiple (rare)
        for sub in ("mag", "satellite", "bag"):
            k = f"pipelines/{sub}/{name}"
            if k in live_pipe:
                return k
    return None


@dataclass
class CopyRecord:
    action: str
    dest: str
    source: str
    live_key: str | None
    lines: int
    stub_score: int
    sha256: str


def safe_copy(src: Path, dest: Path) -> None:
    dest.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(src, dest)


def extract_snapshot_zip() -> Path | None:
    if not SNAPSHOT_ZIP.exists():
        return None
    tmp = Path(tempfile.mkdtemp(prefix="cesarops_snapshot_"))
    with zipfile.ZipFile(SNAPSHOT_ZIP) as z:
        z.extractall(tmp)
    return tmp


def iter_source_py(label: str, root: Path) -> Iterable[tuple[str, Path, str]]:
    """Yield (origin_label, path, logical_key). logical_key: pipelines/... or repo_root/name or flat/name."""
    if not root.exists():
        return
    if root.name == "pipelines" or "pipelines" in str(root) and root.name in (
        "mag",
        "satellite",
        "bag",
    ):
        # laptop-code/pipelines
        for p in root.rglob("*.py"):
            if "__pycache__" in p.parts:
                continue
            rel = p.relative_to(root)
            if rel.parts[0] not in ("mag", "satellite", "bag"):
                continue
            key = str(Path("pipelines") / rel).replace("\\", "/")
            yield label, p, key
        return

    if "cesarops-wreckhunter build" in str(root):
        for p in root.rglob("*.py"):
            if "__pycache__" in p.parts:
                continue
            if p.parent == root:
                yield label, p, f"flat/{p.name}"
            else:
                rel = p.relative_to(root)
                yield label, p, f"flat_sub/{str(rel).replace(chr(92), '/')}"
        return

    if root.name == "programming":
        for p in root.glob("*.py"):
            yield label, p, f"flat/{p.name}"
        return


def main() -> None:
    live_pipe = live_pipeline_files()
    live_root = live_repo_root_py()
    integrate_manifest: list[CopyRecord] = []
    reference_manifest: list[CopyRecord] = []

    # reset output dirs (keep README if we add later)
    if INTEGRATE.exists():
        shutil.rmtree(INTEGRATE)
    if LIVE_REF.exists():
        shutil.rmtree(LIVE_REF)
    INTEGRATE.mkdir(parents=True)
    LIVE_REF.mkdir(parents=True)

    seen_integrate: set[str] = set()
    seen_ref: set[str] = set()

    def consider_file(origin: str, src: Path, key: str, live_key: str | None) -> None:
        nonlocal integrate_manifest, reference_manifest
        try:
            text = src.read_text(encoding="utf-8", errors="replace")
        except OSError:
            return
        h = hashlib.sha256(text.encode("utf-8", errors="replace")).hexdigest()
        a = analyze_py(text)

        # Resolve live_key for flat/
        lk = live_key
        if key.startswith("flat/"):
            name = Path(key).name
            lk = classify_flat_name(name, live_pipe, live_root)

        if lk and lk.startswith("pipelines/"):
            live_p = live_pipe.get(lk)
            if not live_p:
                if promising(a):
                    dest = INTEGRATE / lk
                    sk = f"integrate/{lk}"
                    if sk not in seen_integrate:
                        safe_copy(src, dest)
                        seen_integrate.add(sk)
                        integrate_manifest.append(
                            CopyRecord(
                                "integrate",
                                sk,
                                f"{origin}:{src}",
                                lk,
                                a["lines"],
                                a["stub_score"],
                                h,
                            )
                        )
                return
            if sha256_file(src) == sha256_file(live_p):
                return
            # diverged / similar — reference copy
            rel = lk.replace("pipelines/", "").replace("/", "__")
            base = src.name
            dest_name = f"{rel}__SIMILAR_TO_LIVE__{origin}__{base}"
            dest = LIVE_REF / dest_name
            sk = f"live_reference/{dest_name}"
            if sk in seen_ref:
                return
            safe_copy(src, dest)
            seen_ref.add(sk)
            reference_manifest.append(
                CopyRecord(
                    "live_reference",
                    sk,
                    f"{origin}:{src}",
                    lk,
                    a["lines"],
                    a["stub_score"],
                    h,
                )
            )
            return

        if lk and lk.startswith("repo_root/"):
            live_p = live_root.get(lk)
            if not live_p:
                if promising(a):
                    dest = INTEGRATE / "repo_root" / src.name
                    sk = f"integrate/repo_root/{src.name}"
                    if sk not in seen_integrate:
                        safe_copy(src, dest)
                        seen_integrate.add(sk)
                        integrate_manifest.append(
                            CopyRecord(
                                "integrate",
                                sk,
                                f"{origin}:{src}",
                                lk,
                                a["lines"],
                                a["stub_score"],
                                h,
                            )
                        )
                return
            if sha256_file(src) == sha256_file(live_p):
                return
            dest_name = f"repo_root__{src.name}__SIMILAR_TO_LIVE__{origin}__{src.name}"
            dest = LIVE_REF / dest_name
            sk = f"live_reference/{dest_name}"
            if sk in seen_ref:
                return
            safe_copy(src, dest)
            seen_ref.add(sk)
            reference_manifest.append(
                CopyRecord(
                    "live_reference",
                    sk,
                    f"{origin}:{src}",
                    lk,
                    a["lines"],
                    a["stub_score"],
                    h,
                )
            )
            return

        # No live mapping — integrate if promising
        if promising(a):
            sub = f"unmapped/{origin.replace('-', '_')}"
            dest = INTEGRATE / sub / src.name
            sk = f"integrate/{sub}/{src.name}"
            if sk in seen_integrate:
                return
            safe_copy(src, dest)
            seen_integrate.add(sk)
            integrate_manifest.append(
                CopyRecord(
                    "integrate",
                    sk,
                    f"{origin}:{src}",
                    None,
                    a["lines"],
                    a["stub_score"],
                    h,
                )
            )

    # Walk configured sources
    for label, root in SOURCES:
        if label == "laptopdump-programming-root":
            for p in root.glob("*.py"):
                consider_file(label, p, f"flat/{p.name}", None)
            continue
        if label == "laptopdump-wreckhunter-build":
            for p in root.rglob("*.py"):
                if "__pycache__" in p.parts:
                    continue
                if p.parent == root:
                    consider_file(label, p, f"flat/{p.name}", None)
            continue
        for label2, p, key in iter_source_py(label, root):
            consider_file(label2, p, key, key)

    # Snapshot zip (flat py files)
    snap_root = extract_snapshot_zip()
    if snap_root:
        for p in snap_root.rglob("*.py"):
            if p.name == "__init__.py" and p.read_text(encoding="utf-8", errors="replace").strip() == "":
                continue
            consider_file("CESAROPS_COMPLETE_SNAPSHOT.zip", p, f"flat/{p.name}", None)
        shutil.rmtree(snap_root, ignore_errors=True)

    (INTEGRATE / "_MANIFEST.json").write_text(
        json.dumps([asdict(x) for x in integrate_manifest], indent=2) + "\n",
        encoding="utf-8",
    )
    (LIVE_REF / "_MANIFEST.json").write_text(
        json.dumps([asdict(x) for x in reference_manifest], indent=2) + "\n",
        encoding="utf-8",
    )

    readme = f"""# integrate/

Auto-collected from laptop dump / laptop-code / snapshot zip vs **live**:
- Pipelines: `{LIVE_PIPELINES}`
- Repo root `.py`: `{REPO}/*.py`

**Files:** {len(integrate_manifest)} (see `_MANIFEST.json`)

Next: `python3 -m py_compile` each file, then port paths / imports into live tree or Forge tools.
"""
    (INTEGRATE / "README.md").write_text(readme, encoding="utf-8")

    ref_readme = f"""# live_reference/

Alternate copies of **live** files (same path under pipelines or repo root), from laptop sources.
Filename pattern: `{{mag|satellite|bag}}__{{file}}__SIMILAR_TO_LIVE__{{origin}}__{{basename}}`

The `live_key` field in `_MANIFEST.json` is the live file path key (e.g. `pipelines/mag/foo.py`).

**Files:** {len(reference_manifest)}

Suggested P100 task: for each row, `diff -u` live file vs this copy, then merge or reject.
"""
    (LIVE_REF / "README.md").write_text(ref_readme, encoding="utf-8")

    print("integrate:", len(integrate_manifest), "live_reference:", len(reference_manifest))


if __name__ == "__main__":
    main()
