# integrate/unmapped/laptopdump_wreckhunter_build/generate_db_master_key.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/wreckhunter/tools/generate_db_master_key.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/tools/`.
2. Copy file to `/codebase/projects/pipelines/wreckhunter/tools/generate_db_master_key.py`.
3. Add `cryptography` to `requirements.txt` (pin version).
4. Refactor hardcoded paths:
   - Replace `DB_PATH` with env var `WRECKHUNTER_DB_PATH` or CLI arg.
   - Replace `KEY_OUTPUT` with env var `MASTER_KEY_OUTPUT` or CLI arg.
   - Replace `SALT_FILE` with constant or config.
5. Add unit tests for `generate_master_key` and `get_or_create_salt` using mocks for `find_external_hd`.
6. Add `--help` or usage documentation for the external HD requirement.

## Risks
- Hardcoded paths break portability and CI/CD.
- External HD dependency makes automated testing difficult; requires mocking.
- `subprocess` calls for disk serials are fragile across OS versions.
- `cryptography` version compatibility must be verified.
- Security review needed for key derivation and salt storage logic.
