# integrate/unmapped/laptopdump_wreckhunter_build/check_xenon_cuda.py

## Verdict
ARCHIVE_STUB

## Target path
/codebase/archive/diagnostics/check_xenon_cuda.py

## Steps
1. Move file to `/codebase/archive/diagnostics/check_xenon_cuda.py`
2. Replace hardcoded `XENON_HOST` and `XENON_USER` with `argparse` CLI arguments or a `.env` config loader.
3. Add a minimal `pytest` stub to validate SSH command string formatting without executing network calls.
4. Document deprecation status in `archive/diagnostics/README.md`.

## Risks
- Hardcoded IP prevents reuse across fleet nodes.
- SSH dependency will break in isolated CI/CD environments.
- Loss of quick ad-hoc GPU verification capability for node `10.0.0.55`.
