# integrate/unmapped/laptopdump_wreckhunter_build/repeatability_check.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tests/validation/repeatability_check.py

## Steps
1. Create directory `tests/validation/` in the pipeline root.
2. Refactor `DATA_DIR`, `OUTPUT_BASE`, and `DB_PATH` to be passed via `argparse` instead of hardcoded Windows-style relative paths.
3. Update `run_scanner` to accept the path to `cesarops-search` as a command-line argument.
4. Replace `pathlib` usage with platform-agnostic logic (remove `.\` prefixes).
5. Replace the regex-based `parse_kml` with a robust XML parser (e.g., `lxml` or `xml.etree.ElementTree`) to ensure stability against KML schema variations.
6. Implement a cleanup step to remove the SQLite database after analysis if running in CI.

## Risks
* **Brittle Parsing:** The current regex-based KML parser is highly susceptible to failure if the KML structure or namespace changes.
* **Path Dependency:** The script relies on specific relative directory structures (`.\target\release\`) which will fail in containerized or Linux-based CI environments without refactoring.
* **Data Integrity:** The `analyze()` function assumes `run_id` 1 and 2 exist; if the loop fails early, the analysis will crash.
