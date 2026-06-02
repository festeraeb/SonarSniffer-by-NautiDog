# integrate/unmapped/laptopdump_wreckhunter_build/live_feed_server.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/cesarops/live_feed_server.py

## Steps
1. Move `live_feed_server.py` to `/codebase/projects/pipelines/cesarops/`.
2. Add `flask` and `simplekml` to the fleet's `requirements.txt` or `pyproject.toml`.
3. Replace hardcoded `DB_PATH` with `os.environ.get('CESAROPS_DB_PATH', Path(__file__).parent / "data" / "LAKE_MICHIGAN_CENSUS_2026.db")`.
4. Replace hardcoded `FLASK_SECRET_KEY` and `API_KEYS` with environment variables (`FLASK_SECRET_KEY`, `CESAROPS_API_KEYS_JSON`).
5. Update `sys.path` or package structure to resolve `detection_sorter` imports correctly.
6. Configure the pipeline runner to start the Flask app on port `8080` and expose `/sorter`, `/feed.kmz`, and `/api/*` routes.
7. Add a health check endpoint (e.g., `/api/health`) returning `{"status": "ok"}` for fleet monitoring.
8. Verify database initialization logic runs before server startup or document the dependency on `cesarops_engine.py`.

## Risks
- Hardcoded `API_KEYS` and `FLASK_SECRET_KEY` pose security risks if not migrated to fleet secrets/env vars.
- `simplekml` may not be present in the base fleet image; requires explicit dependency resolution.
- Relative DB path `wreckhunter2000/...` will break in containerized/pipeline environments
