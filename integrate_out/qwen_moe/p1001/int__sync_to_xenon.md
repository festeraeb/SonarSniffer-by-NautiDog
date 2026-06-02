# integrate/unmapped/laptopdump_wreckhunter_build/sync_to_xenon.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/utils/sync_to_xenon.py

## Steps
1. Move file to `/codebase/projects/pipelines/utils/sync_to_xenon.py`.
2. Verify source files (`database_connector.py`, `cesarops_comprehensive_schema.sql`, `init_database.py`) are accessible or update `FILES_TO_SYNC` paths.
3. Add unit tests mocking `subprocess.run` to verify sync logic without network.
4. Document SSH key setup in project README.
5. Verify `scp` availability in target environment.

## Risks
- Hardcoded credentials (`XENON_USER`, `XENON_HOST`).
- SSH key dependency for automation.
- Network dependency.
- Missing source files if moved.
