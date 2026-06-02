# integrate/unmapped/laptopdump_wreckhunter_build/small_batch_test.py

## Verdict
PORT_TO_PIPELINES

## Target path
`/codebase/projects/pipelines/wreckhunter/small_batch_anomaly_test.py`

## Steps
1. **Move & Rename**: Move file to `pipelines/wreckhunter/small_batch_anomaly_test.py`.
2. **Config Injection**: Replace hardcoded `SEARCH_DIR`, `OUTPUT_DIR`, `DB_PATH` with `argparse` or `os.environ` defaults. Add `--max-tiles` flag.
3. **Logging**: Replace `print()` with `logging` module. Add `--log-level` flag.
4. **Path Resolution**: Use `Path(__file__).parent` or fleet root for relative paths. Ensure `DB_PATH` is writable in fleet context.
5. **Pipeline Definition**: Create `pipelines/wreckhunter/small_batch_test.yaml` with:
   - `tool: python`
   - `args: [small_batch_anomaly_test.py, --max-tiles 5]`
   - `inputs: [wreckhunter2000/data/cache/census_raw/2025_rossa]`
   - `outputs: [outputs/small_batch_test/results.json, wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db]`
6. **Tests**: Add `tests/test_small_batch_anomaly.py` with mock tiles and verify DB/JSON output.
7. **Dependencies**: Ensure `numpy`, `Pillow` are in fleet `requirements.txt` or `environment.yml`.

## Risks
- **Hardcoded Paths**: `wreckhunter2000/` paths will break in fleet; must be relative or env-configured.
- **SQLite Concurrency**: `sqlite3` is not safe for parallel writes; pipeline must enforce single-instance or use WAL mode.
- **Memory**: `np.array(img)` loads full tile into RAM; ensure fleet nodes have sufficient memory for large TIFFs.
- **Error Handling**: `process_tile` catches all exceptions; ensure specific errors are logged for debugging.
- **Data Integrity**: `init_db` uses `executescript`; ensure idempotency or migration strategy for schema changes.
