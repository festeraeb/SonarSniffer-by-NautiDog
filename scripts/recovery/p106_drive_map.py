#!/usr/bin/env python3
"""
P106 recovery pipeline: map recovered/lost files → repo affinity + junk tiers.

Phases:
  0. Mechanical — SHA256 dedup, denylisted dirs, extension gates
  1. Heuristic junk — scratch scripts, one-offs, empty stubs (no misused SST-2)
  2. Semantic match — Jina code-v2 embeddings vs clean_repos centroids (optional GPU)

Outputs under var/recovery/:
  forge_file_manifest.json   — per-path verdict
  manifest_summary.json      — counts by category
  nautivecs_ingest.jsonl     — lines for vector DB (path, repo, snippet, category)

Env:
  RECOVERY_CUDA_DEVICE=0     — nvidia-smi index for P106 on cesarops2 (default 0)
  RECOVERY_VENV              — Python with torch + sentence-transformers for --full
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import sys
import time
from pathlib import Path
from typing import Any

REPO = Path(os.environ.get("REPO", "/data/codebase/repos/wreckhunter2000-1"))
OUT_DIR = Path(os.environ.get("RECOVERY_OUT", REPO / "var/recovery"))
CLEAN_REPOS = Path(os.environ.get("CLEAN_REPOS", REPO / "var/recovery/clean_repos"))
def _default_scan_root() -> str:
    for cand in (
        os.environ.get("RECOVERY_SCAN_ROOT", ""),
        "/data/laptopdump",
        "/mnt/t440/data/laptopdump",
    ):
        if cand and Path(cand).is_dir():
            return cand
    return "/mnt/t440/data/laptopdump"


DEFAULT_SCAN = _default_scan_root()

TEXT_EXTS = {
    ".py",
    ".rs",
    ".sh",
    ".bash",
    ".toml",
    ".yaml",
    ".yml",
    ".json",
    ".md",
    ".txt",
    ".js",
    ".ts",
    ".tsx",
    ".jsx",
    ".sql",
    ".service",
    ".nomad",
    ".hcl",
}
BINARY_SKIP_EXTS = {
    ".gguf",
    ".bin",
    ".pt",
    ".pth",
    ".onnx",
    ".zip",
    ".tar",
    ".gz",
    ".tiff",
    ".tif",
    ".nc",
    ".bag",
    ".msi",
    ".dll",
    ".so",
    ".o",
    ".a",
}
DENY_DIR_PARTS = {
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "__pycache__",
    ".cache",
    "target/debug",
    "target/release",
    ".cargo-docker",
}
JUNK_PATH_RE = re.compile(
    r"(oneoff|one-off|_ephemeral|/tmp/|/Temp/|scratch|wget_|curl_|debug_|test_)",
    re.I,
)
JUNK_CONTENT_HINTS = (
    "wget http",
    "curl -",
    "pip install",
    "nohup ",
    "# oneoff",
    "# ephemeral",
    "CESAROPS_EPHEMERAL",
)


def log(msg: str) -> None:
    print(f"[p106-map] {msg}", flush=True)


def file_hash(path: Path) -> str | None:
    h = hashlib.sha256()
    try:
        with path.open("rb") as f:
            for chunk in iter(lambda: f.read(65536), b""):
                h.update(chunk)
    except OSError:
        return None
    return h.hexdigest()


def should_skip_dir(root: str) -> bool:
    parts = Path(root).parts
    return any(d in parts for d in DENY_DIR_PARTS)


def read_snippet(path: Path, limit: int = 4096) -> str:
    try:
        return path.read_text(encoding="utf-8", errors="ignore")[:limit]
    except OSError:
        return ""


def heuristic_junk(path: Path, content: str) -> tuple[bool, str, float]:
    """Rule-based junk detector (replaces mis-purposed DistilBERT SST-2)."""
    rel = str(path).lower()
    if JUNK_PATH_RE.search(rel):
        return True, "JUNK_PATH_PATTERN", 0.9
    if len(content.strip()) < 20:
        return True, "JUNK_EMPTY_OR_STUB", 1.0
    head = content[:800].lower()
    if any(h in head for h in JUNK_CONTENT_HINTS):
        return True, "LIKELY_JUNK_SCRATCHPAD", 0.85
    if path.suffix.lower() == ".sh" and "function " not in content and len(content) < 400:
        if re.search(r"^(wget|curl|pip|apt|echo)\s", content.strip(), re.M):
            return True, "LIKELY_JUNK_SCRATCHPAD", 0.8
    return False, "", 0.0


def load_embedder(device: str):
    from sentence_transformers import SentenceTransformer

    log(f"loading Jina code embedder on {device}...")
    model = SentenceTransformer(
        "jinaai/jina-embeddings-v2-base-code",
        trust_remote_code=True,
    )
    if device.startswith("cuda"):
        model = model.to(device)
    return model


def build_repo_profiles(embedder, clean_root: Path, max_files_per_repo: int = 200) -> dict[str, Any]:
    import numpy as np

    profiles: dict[str, Any] = {}
    if not clean_root.is_dir():
        log(f"warn: clean_repos missing: {clean_root}")
        return profiles

    for repo_dir in sorted(clean_root.iterdir()):
        if not repo_dir.is_dir() and not repo_dir.is_symlink():
            continue
        repo_name = repo_dir.name
        vectors = []
        count = 0
        for root, dirs, files in os.walk(repo_dir):
            if should_skip_dir(root):
                dirs[:] = []
                continue
            for fname in files:
                if count >= max_files_per_repo:
                    break
                p = Path(root) / fname
                if p.suffix.lower() not in TEXT_EXTS:
                    continue
                text = read_snippet(p, 2048)
                if len(text.strip()) < 80:
                    continue
                try:
                    vec = embedder.encode(text, convert_to_numpy=True)
                    vectors.append(vec)
                    count += 1
                except Exception:
                    continue
            if count >= max_files_per_repo:
                break
        if vectors:
            profiles[repo_name] = {
                "centroid": np.mean(vectors, axis=0),
                "samples": len(vectors),
            }
            log(f"  profile {repo_name}: {len(vectors)} samples")
    return profiles


def cosine_sim(a, b) -> float:
    import numpy as np

    na = np.linalg.norm(a)
    nb = np.linalg.norm(b)
    if na == 0 or nb == 0:
        return -1.0
    return float(np.dot(a, b) / (na * nb))


def match_repo(file_vec, profiles: dict[str, Any], threshold: float) -> tuple[str, float]:
    best_name = "UNKNOWN_OR_ORPHAN"
    best_sim = -1.0
    for name, prof in profiles.items():
        sim = cosine_sim(file_vec, prof["centroid"])
        if sim > best_sim:
            best_sim = sim
            best_name = name
    if best_sim >= threshold:
        return f"MATCHES_REPO_{best_name}", best_sim
    return "VALID_CODE_ORPHAN", best_sim


def run_scan(
    scan_root: Path,
    clean_root: Path,
    *,
    full_embed: bool,
    device: str,
    match_threshold: float,
    max_files: int,
) -> dict[str, Any]:
    manifest: dict[str, Any] = {}
    ingest_lines: list[dict[str, Any]] = []
    seen_hashes: set[str] = set()
    profiles: dict[str, Any] = {}
    embedder = None

    if full_embed:
        embedder = load_embedder(device)
        profiles = build_repo_profiles(embedder, clean_root)
        if not profiles:
            log("warn: no repo profiles — semantic match disabled")

    scanned = 0
    for root, dirs, files in os.walk(scan_root):
        if should_skip_dir(root):
            dirs[:] = []
            continue
        for fname in files:
            if max_files and scanned >= max_files:
                break
            path = Path(root) / fname
            scanned += 1
            if scanned % 500 == 0:
                log(f"scanned {scanned} files...")

            ext = path.suffix.lower()
            key = str(path)

            if ext in BINARY_SKIP_EXTS or ext not in TEXT_EXTS:
                fh = file_hash(path) if path.stat().st_size < 50_000_000 else None
                manifest[key] = {
                    "category": "BINARY_OR_LARGE_SKIP",
                    "confidence": 1.0,
                    "sha256": fh,
                }
                continue

            fh = file_hash(path)
            if not fh:
                manifest[key] = {"category": "ERROR_UNREADABLE", "confidence": 0.0}
                continue
            if fh in seen_hashes:
                manifest[key] = {"category": "JUNK_EXACT_DUPLICATE", "confidence": 1.0, "sha256": fh}
                continue
            seen_hashes.add(fh)

            content = read_snippet(path)
            is_junk, junk_cat, junk_conf = heuristic_junk(path, content)
            if is_junk:
                manifest[key] = {
                    "category": junk_cat,
                    "confidence": junk_conf,
                    "sha256": fh,
                }
                continue

            if embedder and profiles:
                try:
                    vec = embedder.encode(content, convert_to_numpy=True)
                    cat, conf = match_repo(vec, profiles, match_threshold)
                    manifest[key] = {
                        "category": cat,
                        "confidence": conf,
                        "sha256": fh,
                    }
                    if cat.startswith("MATCHES_REPO_"):
                        repo = cat.replace("MATCHES_REPO_", "", 1)
                        ingest_lines.append(
                            {
                                "path": key,
                                "repo": repo,
                                "category": cat,
                                "confidence": conf,
                                "snippet": content[:1500],
                            }
                        )
                except Exception as e:
                    manifest[key] = {
                        "category": "ERROR_EMBED",
                        "details": str(e),
                        "sha256": fh,
                    }
            else:
                manifest[key] = {
                    "category": "REVIEW_HEURISTIC_OK",
                    "confidence": 0.5,
                    "sha256": fh,
                }
                ingest_lines.append(
                    {
                        "path": key,
                        "repo": "unknown",
                        "category": "REVIEW_HEURISTIC_OK",
                        "confidence": 0.5,
                        "snippet": content[:1500],
                    }
                )

        if max_files and scanned >= max_files:
            break

    return {
        "manifest": manifest,
        "ingest_lines": ingest_lines,
        "profiles": {k: v["samples"] for k, v in profiles.items()},
        "scanned": scanned,
    }


def summarize(manifest: dict[str, Any]) -> dict[str, int]:
    counts: dict[str, int] = {}
    for rec in manifest.values():
        cat = rec.get("category", "UNKNOWN")
        counts[cat] = counts.get(cat, 0) + 1
    return dict(sorted(counts.items(), key=lambda x: -x[1]))


def main() -> int:
    ap = argparse.ArgumentParser(description="P106 recovered-drive mapper")
    ap.add_argument("--scan", default=DEFAULT_SCAN, help="Root to scan (recovered drive)")
    ap.add_argument("--clean-repos", default=str(CLEAN_REPOS))
    ap.add_argument("--out", default=str(OUT_DIR))
    ap.add_argument("--rules-only", action="store_true", help="Skip Jina GPU embedder")
    ap.add_argument("--full", action="store_true", help="Use Jina on GPU (needs torch)")
    ap.add_argument("--match-threshold", type=float, default=0.62)
    ap.add_argument("--max-files", type=int, default=0, help="0 = unlimited")
    ap.add_argument("--device", default=os.environ.get("RECOVERY_CUDA_DEVICE", "0"))
    args = ap.parse_args()

    out_dir = Path(args.out)
    out_dir.mkdir(parents=True, exist_ok=True)
    scan_root = Path(args.scan)
    if not scan_root.is_dir():
        log(f"ERROR: scan root missing: {scan_root}")
        return 1

    full_embed = args.full and not args.rules_only
    device = "cpu"
    if full_embed:
        import torch

        if not torch.cuda.is_available():
            log("CUDA unavailable — use --rules-only or install torch+cuda")
            return 1
        idx = int(args.device)
        device = f"cuda:{idx}"
        log(f"using GPU {torch.cuda.get_device_name(idx)} as {device}")

    t0 = time.time()
    result = run_scan(
        scan_root,
        Path(args.clean_repos),
        full_embed=full_embed,
        device=device,
        match_threshold=args.match_threshold,
        max_files=args.max_files,
    )

    manifest_path = out_dir / "forge_file_manifest.json"
    summary_path = out_dir / "manifest_summary.json"
    ingest_path = out_dir / "nautivecs_ingest.jsonl"
    profiles_path = out_dir / "repo_profiles_built.json"

    manifest_path.write_text(json.dumps(result["manifest"], indent=2), encoding="utf-8")
    summary = {
        "scanned": result["scanned"],
        "elapsed_s": round(time.time() - t0, 1),
        "scan_root": str(scan_root),
        "clean_repos": args.clean_repos,
        "mode": "full_jina" if full_embed else "rules_only",
        "counts": summarize(result["manifest"]),
        "repo_profile_samples": result["profiles"],
    }
    summary_path.write_text(json.dumps(summary, indent=2), encoding="utf-8")
    with ingest_path.open("w", encoding="utf-8") as f:
        for line in result["ingest_lines"]:
            f.write(json.dumps(line, ensure_ascii=False) + "\n")
    profiles_path.write_text(json.dumps(result["profiles"], indent=2), encoding="utf-8")

    log(f"manifest → {manifest_path}")
    log(f"summary  → {summary_path}")
    log(f"ingest   → {ingest_path} ({len(result['ingest_lines'])} lines)")
    log(f"top categories: {list(summary['counts'].items())[:8]}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
