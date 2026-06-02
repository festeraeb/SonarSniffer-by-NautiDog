# integrate/unmapped/laptopdump_wreckhunter_build/raw_scan_reprocess.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/ops/raw_scan_reprocess.py

## Steps
1. Copy `raw_scan_reprocess.py` to `/codebase/projects/pipelines/ops/raw_scan_reprocess.py`.
2. Replace `os.chdir(ROOT)` with explicit path resolution or rely on `PYTHONPATH` to prevent environment pollution.
3. Update `subprocess.run([sys.executable, 'hard_pixel_audit.py'])` to import `hard_pixel_audit` directly from `wreckhunter2000` or resolve its absolute path via `pkg_resources`/`importlib`.
4. Ensure `wreckhunter2000` is listed in `requirements.txt` or `pyproject.toml` dependencies for the fleet environment.
5. Register CLI entry point in `pyproject.toml` under `[project.scripts]` (e.g., `raw-scan-reprocess = "ops.raw_scan_reprocess:main"`).
6. Run `pytest` against `ops/raw_scan_reprocess.py` with mocked `ensure_tiles` and `hard_pixel_audit` to verify argument parsing and mode routing.
7. Add to CI pipeline as a manual-triggered job or scheduled cron for reprocessing workflows.

## Risks
- `wreckhunter2000` module may not be installed in the target fleet environment.
- `hard_pixel_audit.py` path resolution via subprocess is fragile; direct import is preferred.
- `os.chdir(ROOT)` removal may break relative path expectations in downstream `wreckhunter2000` calls if they rely on cwd.
- CLI argument conflicts with existing pipeline runners if registered globally.
