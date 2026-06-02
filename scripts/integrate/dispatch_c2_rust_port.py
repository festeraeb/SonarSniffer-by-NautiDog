#!/usr/bin/env python3
"""Dispatch Python→Rust port plans to cesarops2 dual LLMs (2/3 Qwen :5200, 1/3 Rust :5571)."""
from __future__ import annotations

import json
import os
import textwrap
import urllib.error
import urllib.request
from pathlib import Path

REPO = Path(os.environ.get("REPO", "/codebase/repos/wreckhunter2000-1"))
PIPE_ROOTS = [
    Path("/codebase/projects/pipelines"),
    Path("/mnt/t440/codebase/projects/pipelines"),
]
OUT_QWEN = Path(os.environ.get("OUT_QWEN", REPO / "integrate_out" / "qwen14b_2060"))
OUT_RUST = Path(os.environ.get("OUT_RUST", REPO / "integrate_out" / "rust14b_1070"))
LOG = Path(os.environ.get("LOG", "/tmp/c2-rust-port-dispatch.log"))
CODER_URL = os.environ.get("CODER_URL", "http://127.0.0.1:5200/v1/chat/completions")
RUST_URL = os.environ.get("RUST_URL", "http://127.0.0.1:5571/v1/chat/completions")
MAX_BODY = int(os.environ.get("MAX_BODY_CHARS", "14000"))
MAX_TOKENS = int(os.environ.get("MAX_TOKENS", "4096"))
TIMEOUT = int(os.environ.get("TIMEOUT", "600"))

# Already ported in cesarops-inference/src/integrate/ (skip)
SKIP_RUST = {
    "cuda_env.rs",
    "cuda_verification.rs",
    "global_controls.rs",
    "find_swot_dates.rs",
    "repeatability_check.rs",
    "tpu_client.rs",
    "three_tile_offset.rs",
    "db_master_key.rs",
    "generate_db_master_key.rs",
    "run_zero_baseline.rs",
    "run_zero.rs",
    "database_connector.rs",
    "init_database.rs",
    "audit_wrecks_db.rs",
    "fetcher.rs",
    "bridge_calibrate.rs",
    "extract_oil_spills_kmz.rs",
    "gpu_health.rs",
    "gpu_stress_test.rs",
    "sync_xenon.rs",
    "xenon_sync.rs",
    "tpu_server.rs",
    "geotiff_inventory.rs",
    "file_inventory.rs",
    "hls_b02_download.rs",
    "batch_download_manager.rs",
    "cuda_test_kmz.rs",
    "db_ingestor.rs",
    "erie_multiyear_downloader.rs",
    "full_lake_michigan_run.rs",
    "full_scan.rs",
    "iowa_202_analysis.rs",
    "live_feed_server.rs",
    "monster_analysis.rs",
    "monster_candidate.rs",
    "populate_database.rs",
    "prioritized_satellite_pull.rs",
    "process_tiles.rs",
    "process_with_coordinates.rs",
    "pull_altimetry_anonymous.rs",
    "raw_scan_reprocess.rs",
    "run_configured_pipeline.rs",
    "straits_fox_pipeline.rs",
    "small_batch_test.rs",
    "smart_daily_scan.rs",
    "gpu_detection.rs",
    "m2200_gpu_test.rs",
    "gpu_test.rs",
    "pipeline_test.rs",
    "test_repeatability.rs",
    "thermal_validation.rs",
    "viirs_multi_year_scan.rs",
    "zion_trench_squeeze.rs",
    "fuel_leak.rs",
    "cesarops_orchestrator.rs",
    "sensor_scan.rs",
    "great_lakes.rs",
    "cuda_stats.rs",
    "forensic_scan.rs",
    "full_basin.rs",
    "triple_lock.rs",
    "daily_pull.rs",
    "cli_orchestrator.rs",
    "agent_presets.rs",
    "ai_director.rs",
    "crossref.rs",
    "drive_identity.rs",
    "dynamic_key.rs",
    "lake_michigan_dual_scan.rs",
    "detection_sorter.rs",
    "tiff_fast.rs",
    "resolution_comparison.rs",
    "anchor_lock_display.rs",
    "xenon_cuda_checker.rs",
    "daily_scan.rs",
    "gpu_diagnostic.rs",
    "lake_michigan_scan.rs",
    "hard_pixel_audit.rs",
    "monster_material_audit.rs",
    "andaste_geometry.rs",
}

# id, python_rel, rust_rel
PORT_TABLE: list[tuple[int, str, str]] = [
    (1, "wreckhunter/tools/audit_wrecks_db.py", "cesarops-inference/src/integrate/audit_wrecks_db.rs"),
    (2, "satellite/b02_download.py", "cesarops-inference/src/integrate/hls_b02_download.rs"),
    (3, "wreckhunter/batch_download_manager.py", "cesarops-inference/src/integrate/batch_download_manager.rs"),
    (4, "wreckhunter/bridge_calibrate.py", "cesarops-inference/src/integrate/bridge_calibrate.rs"),
    (5, "tools/cuda_env.py", "cesarops-inference/src/integrate/cuda_env.rs"),
    (6, "tests/benchmarks/cuda_test_kmz.py", "cesarops-inference/src/integrate/cuda_test_kmz.rs"),
    (7, "utils/database_connector.py", "cesarops-inference/src/integrate/database_connector.rs"),
    (8, "wreckhunter/db_ingestor.py", "cesarops-inference/src/integrate/db_ingestor.rs"),
    (9, "wreckhunter/download_erie_multiyear.py", "cesarops-inference/src/integrate/erie_multiyear_downloader.rs"),
    (10, "leaking_boat/extract_oil_spills_kmz.py", "cesarops-inference/src/integrate/extract_oil_spills_kmz.rs"),
    (11, "wreckhunter/fetcher.py", "cesarops-inference/src/integrate/fetcher.rs"),
    (12, "satellite/swot/find_swot_dates.py", "cesarops-inference/src/integrate/find_swot_dates.rs"),
    (13, "wreckhunter/full_lake_michigan_run.py", "cesarops-inference/src/integrate/full_lake_michigan_run.rs"),
    (14, "wreckhunter/full_scan.py", "cesarops-inference/src/integrate/full_scan.rs"),
    (15, "wreckhunter/tools/generate_db_master_key.py", "cesarops-inference/src/integrate/generate_db_master_key.rs"),
    (16, "global_controls.py", "cesarops-inference/src/integrate/global_controls.rs"),
    (17, "benchmarks/gpu_stress_test.py", "cesarops-inference/src/integrate/gpu_stress_test.rs"),
    (18, "cesarops/db/init_database.py", "cesarops-inference/src/integrate/init_database.rs"),
    (19, "archives/wreckhunter_recovery/inventory_all_files.py", "cesarops-inference/src/integrate/file_inventory.rs"),
    (20, "wreckhunter/tasks/inventory_geotiffs.py", "cesarops-inference/src/integrate/geotiff_inventory.rs"),
    (21, "wreckhunter/iowa_202_analysis.py", "cesarops-inference/src/integrate/iowa_202_analysis.rs"),
    (22, "cesarops/live_feed_server.py", "cesarops-inference/src/integrate/live_feed_server.rs"),
    (23, "analysis/monster_site_analysis.py", "cesarops-inference/src/integrate/monster_analysis.rs"),
    (24, "analysis/monster_candidate.py", "cesarops-inference/src/integrate/monster_candidate.rs"),
    (25, "wreckhunter/populate_database.py", "cesarops-inference/src/integrate/lake_michigan/populate_database.rs"),
    (26, "wreckhunter/prioritized_satellite_pull.py", "cesarops-inference/src/integrate/prioritized_satellite_pull.rs"),
    (27, "wreckhunter/process_tiles.py", "cesarops-inference/src/integrate/process_tiles.rs"),
    (28, "processing/anomaly_extractor.py", "cesarops-inference/src/integrate/process_with_coordinates.rs"),
    (29, "wreckhunter/pull_altimetry_anonymous.py", "cesarops-inference/src/integrate/pull_altimetry_anonymous.rs"),
    (30, "wreckhunter/tools/raw_scan_reprocess.py", "cesarops-inference/src/integrate/raw_scan_reprocess.rs"),
    (31, "tools/repeatability_check.py", "cesarops-inference/src/integrate/repeatability_check.rs"),
    (32, "wreckhunter/run_configured_pipeline.py", "cesarops-inference/src/integrate/run_configured_pipeline.rs"),
    (33, "wreckhunter/straits_fox_runner.py", "cesarops-inference/src/integrate/straits_fox_pipeline.rs"),
    (34, "benchmarks/run_zero.py", "cesarops-inference/src/integrate/run_zero.rs"),
    (35, "wreckhunter/small_batch_anomaly_test.py", "cesarops-inference/src/integrate/small_batch_test.rs"),
    (36, "wreckhunter/smart_daily_scan.py", "cesarops-inference/src/integrate/smart_daily_scan.rs"),
    (37, "wreckhunter/sync_xenon_db.py", "cesarops-inference/src/integrate/xenon_sync.rs"),
    (38, "deploy_xenon.py", "cesarops-inference/src/integrate/sync_xenon.rs"),
    (39, "tests/hardware/test_cuda_minimal.py", "cesarops-inference/src/integrate/gpu_health.rs"),
    (40, "tests/hardware/test_gpu_validation.py", "cesarops-inference/src/integrate/gpu_detection.rs"),
    (41, "gpu_engine/tests/test_m2200_hardware.py", "cesarops-inference/src/integrate/m2200_gpu_test.rs"),
    (42, "cesarops-gpu/tests/test_m2200_minimal.py", "cesarops-inference/src/integrate/gpu_test.rs"),
    (43, "tests/test_end_to_end_gpu.py", "cesarops-inference/src/integrate/pipeline_test.rs"),
    (44, "tests/test_repeatability.py", "cesarops-inference/src/integrate/test_repeatability.rs"),
    (45, "wreckhunter/tools/three_tile_offset_analysis.py", "cesarops-inference/src/integrate/three_tile_offset_analysis.rs"),
    (46, "utils/tpu_client.py", "cesarops-inference/src/integrate/tpu_client.rs"),
    (47, "wreckhunter/tpu_server.py", "cesarops-inference/src/integrate/tpu_server.rs"),
    (48, "wreckhunter/tests/validate_detection.py", "cesarops-inference/src/integrate/thermal_validation.rs"),
    (49, "wreckhunter/utils/verify_cuda.py", "cesarops-inference/src/integrate/cuda_verification.rs"),
    (50, "forensics/viirs_multi_year_scan.py", "cesarops-inference/src/integrate/viirs_multi_year_scan.rs"),
    (51, "missions/zion_trench_squeeze.py", "cesarops-inference/src/integrate/zion_trench_squeeze.rs"),
]

BATCH_ORDER = [
    [5, 7, 15, 18, 46, 49, 39, 40, 42],
    [2, 3, 9, 11, 26, 37, 38],
    [1, 8, 13, 14, 20, 21, 23, 24, 29, 30, 36, 50, 51],
    [4, 6, 17, 27, 28, 32, 33, 41, 43, 44, 45, 47, 48],
    [10, 12, 16, 19, 22, 25, 31, 34, 35],
]

SYSTEM = """You are a senior CESAROPS Rust engineer.
Convert the Python module to production Rust for cesarops-inference.

Output markdown ONLY:
## Verdict
ONE of: PORT_TO_PIPELINES | MERGE_INTO_LIVE | ARCHIVE_STUB
## Rust path
Exact path under repo (e.g. cesarops-inference/src/integrate/foo.rs)
## Rust source
Full ```rust ... ``` module: pub fn API, unit tests, minimal deps, match style of existing integrate/*.rs
## mod.rs wire
pub mod line to add in cesarops-inference/src/integrate/mod.rs
## Risks
Brief bullets
Start with ## Verdict. No chain-of-thought."""


def log(msg: str) -> None:
    line = f"[c2-rust-port] {msg}"
    print(line, flush=True)
    with LOG.open("a", encoding="utf-8") as f:
        f.write(line + "\n")


def resolve_python(rel: str) -> Path | None:
    for root in PIPE_ROOTS:
        p = root / rel
        if p.is_file():
            return p
    return None


def ordered_items() -> list[tuple[int, str, str]]:
    by_id = {i: (i, py, rs) for i, py, rs in PORT_TABLE}
    out: list[tuple[int, str, str]] = []
    seen: set[int] = set()
    for batch in BATCH_ORDER:
        for i in batch:
            if i in by_id and i not in seen:
                out.append(by_id[i])
                seen.add(i)
    for i in sorted(by_id):
        if i not in seen:
            out.append(by_id[i])
    return out


def assign_gpu(index: int) -> str:
    """2/3 Qwen (2060), 1/3 Rust specialist (1070)."""
    return "rust" if index % 3 == 2 else "qwen"


def chat(url: str, user: str) -> str:
    body = {
        "model": "x",
        "messages": [
            {"role": "system", "content": SYSTEM},
            {"role": "user", "content": user},
        ],
        "max_tokens": MAX_TOKENS,
        "temperature": 0.15,
    }
    req = urllib.request.Request(
        url,
        data=json.dumps(body).encode(),
        headers={"Content-Type": "application/json"},
        method="POST",
    )
    with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
        data = json.loads(resp.read().decode())
    return data["choices"][0]["message"].get("content") or ""


def main() -> None:
    OUT_QWEN.mkdir(parents=True, exist_ok=True)
    OUT_RUST.mkdir(parents=True, exist_ok=True)
    LOG.write_text("", encoding="utf-8")

    items = ordered_items()
    results = []
    qwen_n = rust_n = skip_n = 0

    for idx, (item_id, py_rel, rust_rel) in enumerate(items):
        rust_name = Path(rust_rel).name
        rust_path = REPO / rust_rel
        if rust_name in SKIP_RUST or rust_path.is_file():
            log(f"SKIP already-ported #{item_id} {rust_name}")
            skip_n += 1
            continue

        py_path = resolve_python(py_rel)
        if py_path is None:
            log(f"SKIP missing source #{item_id} {py_rel}")
            results.append({"id": item_id, "error": "missing python", "py": py_rel})
            continue

        base = Path(py_rel).name.replace(".py", "")
        gpu = assign_gpu(idx)
        out_dir = OUT_RUST if gpu == "rust" else OUT_QWEN
        out_md = out_dir / f"rust__{base}.md"

        if out_md.is_file():
            t = out_md.read_text(encoding="utf-8", errors="replace")
            if len(t) > 400 and "## Verdict" in t and "```rust" in t:
                log(f"SKIP done #{item_id} {base} ({gpu})")
                results.append({"id": item_id, "gpu": gpu, "out": str(out_md), "skipped": True})
                continue

        body = py_path.read_text(encoding="utf-8", errors="replace")[:MAX_BODY]
        user = textwrap.dedent(
            f"""
            Convert this merged Python module to Rust for cesarops-inference.
            Python path: {py_path}
            Target Rust path: {REPO / rust_rel}
            Match existing modules under cesarops-inference/src/integrate/ (pub fn API, unit tests, minimal deps).
            --- source ---
            {body}
            """
        ).strip()

        url = RUST_URL if gpu == "rust" else CODER_URL
        log(f"#{item_id} {base} → {gpu} ({url})")
        try:
            reply = chat(url, user)
            if len(reply) < 120 or "## Verdict" not in reply:
                raise ValueError(f"short/invalid reply ({len(reply)} bytes)")
            out_md.write_text(f"# {py_rel}\n\n{reply}\n", encoding="utf-8")
            results.append({"id": item_id, "gpu": gpu, "py": py_rel, "rust": rust_rel, "out": str(out_md)})
            if gpu == "rust":
                rust_n += 1
            else:
                qwen_n += 1
        except Exception as e:
            log(f"ERR #{item_id} {base}: {e}")
            results.append({"id": item_id, "gpu": gpu, "error": str(e)})

    summary = {
        "qwen_out": str(OUT_QWEN),
        "rust_out": str(OUT_RUST),
        "completed_qwen": qwen_n,
        "completed_rust": rust_n,
        "skipped_existing": skip_n,
        "results": results,
    }
    (OUT_QWEN / "dispatch_summary.json").write_text(
        json.dumps(summary, indent=2) + "\n", encoding="utf-8"
    )
    log(f"done qwen={qwen_n} rust={rust_n} skip={skip_n} → {OUT_QWEN}/dispatch_summary.json")


if __name__ == "__main__":
    main()
