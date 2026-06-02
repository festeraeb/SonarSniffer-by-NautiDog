# integrate/unmapped/laptopdump_wreckhunter_build/dynamic_db_key.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/forge_tools/dynamic_db_key.py

## Steps
1. Create `/codebase/projects/pipelines/forge_tools/dynamic_db_key.py`.
2. Replace hardcoded `HD_MOUNT_POINTS`, `XENON_HOST`, and `DB_PATH` with environment variables or a `config.yaml` loader.
3. Refactor `get_disk_serial` to use `psutil.disk_partitions()` or `platform` module for robust cross-platform detection.
4. Convert `generate_dynamic_key` to a pure function accepting `hd_info`, `network_info`, and `connectivity` dicts for testability.
5. Add `pytest` suite mocking `socket`, `subprocess`, and `Path` to verify key generation and access level logic.
6. Register in `/codebase/projects/pipelines/forge_tools/manifest.json` with `requires_external_token: true` flag.

## Risks
- External HD dependency will cause silent failures or timeouts on nodes without the physical token.
- Heuristic network detection (`192.168.x`, `10.x`) is unreliable in NATed or multi-homed environments.
- Disk serials are spoofable and not cryptographically secure; consider replacing with signed JWT or hardware-backed attestation.
- Hardcoded `10.0.0.55` Xenon ping is fragile; should use DNS SRV or service mesh discovery.
- `subprocess` usage for `vol`, `lsblk`, `diskutil`
