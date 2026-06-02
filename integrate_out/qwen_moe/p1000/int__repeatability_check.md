# integrate/unmapped/laptopdump_wreckhunter_build/repeatability_check.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tools/repeatability_check.py

## Steps
1.  **Move & Rename**: Move file to `/codebase/projects/pipelines/tools/repeatability_check.py`.
2.  **Path Abstraction**: Replace hardcoded Windows paths (`r".\target\..."`, `r".\wreckhunter..."`) with configuration via environment variables or a `config.yaml`. Use `Path(__file__).parent` for relative paths where appropriate.
3.  **Binary Resolution**: Update `cesarops-search.exe` reference to resolve via `PATH` or a configurable `BINARY_PATH` env var. Ensure it works on Linux/WSL environments.
4.  **KML Parsing**: Replace fragile regex KML parsing with `xml.etree.ElementTree` or switch scanner output to JSON (preferred) and parse that.
5.  **Database**: Ensure SQLite DB path is configurable. Add connection locking or use a temp DB for parallel runs.
6.  **Testing**: Add `pytest` suite mocking `subprocess.run` to verify detection parsing and DB logging logic without running the heavy scanner.
7.  **Documentation**: Add `README.md` explaining usage for regression testing.

## Risks
*   **KML Fragility**: Regex parsing of KML is brittle; schema changes in scanner output will break this silently.
*   **Platform Dependency**: Current paths are Windows-specific; Linux integration requires careful path handling.
*   **Performance**: SQLite writes per detection are slow; consider batching or in-memory comparison for large runs.
*   **Scope Creep**: This is a dev tool, not a pipeline step; ensure it doesn't bloat the main repo without clear separation.
