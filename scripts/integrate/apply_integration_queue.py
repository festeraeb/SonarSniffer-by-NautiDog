#!/usr/bin/env python3
"""Apply pending integrate queue: MERGE copies, ARCHIVE placement, queue status updates."""
from __future__ import annotations

import json
import re
import shutil
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
QUEUE = REPO / "integrate" / "RUST_PORT_QUEUE.json"
MANIFEST = REPO / "integrate" / "_MANIFEST.json"
UNMAPPED = REPO / "integrate" / "unmapped"
PIPE = Path("/codebase/projects/pipelines")
CODEBASE = Path("/codebase")
LOG = Path("/tmp/apply_integration_queue.log")

FALLBACK_TARGETS = {
    "cuda_env.py": "/codebase/projects/pipelines/tools/cuda_env.py",
    "audit_wrecks_db.py": "/codebase/projects/pipelines/wreckhunter/tools/audit_wrecks_db.py",
    "b02_download.py": "/codebase/projects/pipelines/satellite/b02_download.py",
    "batch_download_manager.py": "/codebase/projects/pipelines/wreckhunter/batch_download_manager.py",
    "bridge_calibrate.py": "/codebase/projects/pipelines/wreckhunter/bridge_calibrate.py",
    "db_ingestor.py": "/codebase/projects/pipelines/wreckhunter/db_ingestor.py",
    "download_erie_multiyear.py": "/codebase/projects/pipelines/wreckhunter/download_erie_multiyear.py",
}


def resolve_target(entry: dict) -> str:
    raw = (entry.get("target") or "").strip()
    if raw:
        return raw
    base = entry["source_py"].replace(".py", "")
    for md in (REPO / "integrate_out").rglob(f"int__{base}.md"):
        text = md.read_text(encoding="utf-8", errors="replace")
        m = re.search(r"^## Target path\s*\n(.+)", text, re.M)
        if m and m.group(1).strip():
            return m.group(1).strip()
    return FALLBACK_TARGETS.get(entry["source_py"], "")


def clean_target(raw: str) -> Path | None:
    if not raw:
        return None
    t = raw.strip().strip("`").strip()
    if t.startswith("/codebase/projects/pipelines/"):
        return PIPE / t.replace("/codebase/projects/pipelines/", "", 1)
    if t.startswith("/codebase/"):
        return Path(t)
    if t.startswith("archive/"):
        return CODEBASE / t
    if t.startswith("integrate/"):
        return REPO / t
    if t.startswith("tools/"):
        return PIPE / t
    return None


def find_source(source_py: str) -> Path | None:
    hits = list(UNMAPPED.rglob(source_py))
    if not hits:
        return None
    # Prefer wreckhunter_build over programming_root over snapshot zip path
    hits.sort(key=lambda p: (
        "wreckhunter_build" not in str(p),
        "programming_root" not in str(p),
        "SNAPSHOT" in str(p),
        len(str(p)),
    ))
    return hits[0]


def patch_windows_paths(text: str) -> str:
    text = re.sub(
        r'Path\(r"C:\\Users\\thomf\\[^"]+"\)',
        'Path(os.environ.get("CESAROPS_DATA_DIR", "/codebase/projects/pipelines/data"))',
        text,
    )
    text = text.replace("C:\\Users\\thomf\\", "/codebase/projects/pipelines/data/")
    if "import os" not in text.split("\n", 20):
        text = "import os\n" + text
    return text


def apply_merge(entry: dict) -> dict:
    src_name = entry["source_py"]
    src = find_source(src_name)
    target = resolve_target(entry)
    dest = clean_target(target)
    if not src or not src.is_file():
        return {"ok": False, "reason": f"missing source {src_name}"}
    if not dest:
        return {"ok": False, "reason": f"bad target {target!r}"}
    dest.parent.mkdir(parents=True, exist_ok=True)
    body = src.read_text(encoding="utf-8", errors="replace")
    body = patch_windows_paths(body)
    dest.write_text(body, encoding="utf-8")
    return {"ok": True, "dest": str(dest), "source": str(src)}


def apply_archive(entry: dict) -> dict:
    src = find_source(entry["source_py"])
    dest = clean_target(resolve_target(entry))
    if not src:
        return {"ok": False, "reason": "missing source"}
    if dest:
        dest.parent.mkdir(parents=True, exist_ok=True)
        shutil.copy2(src, dest)
        return {"ok": True, "dest": str(dest), "action": "copied"}
    # No target — record only
    return {"ok": True, "action": "marked_archived", "source": str(src)}


def main() -> None:
    queue = json.loads(QUEUE.read_text(encoding="utf-8"))
    results = []
    for entry in queue:
        if entry.get("rust_status") != "pending":
            continue
        verdict = entry.get("verdict", "")
        src = entry["source_py"]
        if verdict == "ARCHIVE_STUB":
            r = apply_archive(entry)
            if r.get("ok"):
                entry["rust_status"] = "archived"
                entry["apply_note"] = r
            results.append({"source": src, "verdict": verdict, **r})
        elif verdict == "MERGE_INTO_LIVE":
            r = apply_merge(entry)
            if r.get("ok"):
                entry["rust_status"] = "merged"
                entry["apply_note"] = r
            results.append({"source": src, "verdict": verdict, **r})
        elif verdict == "KEEP_LIVE":
            entry["rust_status"] = "keep_live"
            results.append({"source": src, "verdict": verdict, "ok": True, "action": "no_copy"})
        elif verdict == "PORT_TO_PIPELINES":
            r = apply_merge(entry)
            if r.get("ok"):
                entry["rust_status"] = "merged"
                entry["apply_note"] = r
            results.append({"source": src, "verdict": verdict, **r})

    QUEUE.write_text(json.dumps(queue, indent=2) + "\n", encoding="utf-8")
    ok = sum(1 for r in results if r.get("ok"))
    fail = [r for r in results if not r.get("ok")]
    summary = f"apply_integration_queue: {ok} ok, {len(fail)} fail, {len(results)} touched\n"
    LOG.write_text(summary + json.dumps(results, indent=2) + "\n", encoding="utf-8")
    print(summary.strip())
    for r in fail[:10]:
        print(f"  FAIL {r['source']}: {r.get('reason')}")
    for r in results:
        if r.get("ok") and r.get("dest"):
            print(f"  OK {r['source']} -> {r['dest']}")


if __name__ == "__main__":
    main()
