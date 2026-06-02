# integrate/unmapped/laptopdump_wreckhunter_build/cesarops_engine.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/cesarops/cesarops_engine.py

## Steps
1. **Config & Path Abstraction**: Replace hardcoded `c:\Users\thomf\...` and drive-letter scanning with environment variables (`CESAROPS_DATA_DIR`, `CESAROPS_DB_PATH`) and pipeline mount detection. Add Linux/WSL path resolution.
2. **Environment Compatibility**: Remove `sys.stdout.reconfigure(encoding='utf-8')` Windows workaround. Add explicit Linux console encoding fallback. Validate Python 3.9+ runtime requirements.
3. **TPU Client Wiring**: Replace direct `from tpu_client import TPUClient` with pipeline-registered import (`from cesarops.core.tpu_client import TPUClient`). Inject `TPU_SERVER_URL` via pipeline config or service discovery instead of static string.
4. **GPU/CuPy Fleet Alignment**: Verify CuPy version matches P1000 CUDA toolkit. Add explicit `cp.cuda.runtime.getDeviceCount()` check before processing. Ensure `cp.get_default_memory_pool().free_all_blocks()` is wrapped in try/finally to prevent VRAM leaks in long-running workers.
5. **Pipeline Worker Mode**: Refactor `main()` and `run_test_pipeline()` into a CLI entry point (`cesarops-engine --manifest <path>`). Accept tile paths via JSONL stdin or CLI args. Output structured JSON results to stdout for pipeline downstream consumption.
6. **Database Concurrency**: Replace direct `sqlite3.connect()` in `write_detection()` with connection pooling or pipeline DB connector. Add WAL mode and atomic transaction commits to prevent lock contention across parallel workers.
7. **Test Suite**: Add unit tests for `gpu_process_tile` (mock CuPy arrays), `tpu_glint_check` (mock TPUClient responses), and DB write logic. Add integration test for pipeline worker mode with sample `.tif` fixtures.

## Risks
- Hardcoded Windows paths and drive-letter scanning will fail on Linux fleet nodes; requires immediate config injection.
- CuPy version/driver mismatch on P1000 nodes may cause silent GPU fallback or segfaults; requires explicit CUDA toolkit validation.
- Static `TPU_SERVER_URL` breaks in containerized pipeline environments; needs DNS/service discovery or config map injection.
- Direct SQLite writes in parallel workers will cause `database is locked` errors; requires WAL mode or pipeline-backed storage.
- Missing internal dependencies (`TPUClient`, `wgpu`, `cupy`) must be resolved in pipeline requirements before deployment.
