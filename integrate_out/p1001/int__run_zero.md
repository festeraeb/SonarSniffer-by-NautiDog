# integrate/unmapped/laptopdump_wreckhunter_build/run_zero.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/tests/integration/test_scanner_repeatability.py

## Steps
1. Extract `parse_kml` logic into `utils/kml_parser.py`, replacing brittle regex with `lxml` or `xml.etree`.
2. Port the `runs` and `detections` SQLite schema and logging functions to `core/telemetry.py`.
3. Refactor `get_system_info` into a hardware telemetry utility for performance tracking.
4. Implement the `analyze` comparison logic as a formal integration test within the pipeline's test suite.
5. Replace hardcoded Windows paths and `.exe` calls with a configuration-driven CLI wrapper compatible with Linux environments.

## Risks
* **Brittle Parsing:** The current regex-based KML parsing will fail if the KML schema changes slightly.
* **Platform Dependency:** Hardcoded Windows paths and `.exe` binaries are incompatible with Linux-based pipeline runners.
* **Manual Workflow:** The script is designed for manual execution; it requires a complete overhaul to function as an automated CI/CD integration test.
