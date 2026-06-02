# Laptop dump — archives inside `/data/laptopdump`

**Scanned:** 2026-05-26. Fourteen `.zip` / `.tar.gz` files under `/data/laptopdump` (178 GB tree total).

## Readable archives (can list and extract)

| Archive | Size | Contents summary |
|---------|------|------------------|
| `programming/cesarops-core/Documents/CESAROPS_COMPLETE_SNAPSHOT.zip` | 0.3 MB | **140 files** — flat Python + MD snapshot (82 `.py`, 41 `.md`): `cesarops_engine.py`, `lake_michigan_scan.py`, `triple_lock_fusion.py`, etc. |
| `programming/cesarops-core/Documents/CESAROPS_FULL_BACKUP.zip` | 0.7 MB | **231 files** — `cesarops-clean/`, `cesarops-titan-extracted/`, `outputs/file_inventory.json`, Rust `src/`, `TODO_RECOVERY.md`, `FRESH_START_PLAN.md` |
| `programming/cesarops-core/Documents/CESAROPS_TITAN_SNAPSHOT.zip` | 0.07 MB | **29 files** — Tauri/React `src/` + core Python (`cesarops_engine.py`, `triple_lock_fusion.py`, …) |
| `programming/cesarops-wreckhunter build/` (unpacked) | ~1.1 GB dir | Same era as broken zip below — **81 top-level `.py`** already extracted beside the zip |
| `Temp/nauticuvs.tar.gz` | 28 KB | **nauticuvs-0.1.2** Rust crate (curvelets) |
| `Temp/ort_dlls/ort.zip`, `Temp/ort_dml/ort.zip` | 58 / 14 MB | ONNX Runtime DLL bundles |
| `SonarSniffer/test files/files (1).zip` | 20 KB | Sonar fix notes + `channel_alignment_FIXED.rs` |
| `programming/.../CESAROPS_0.1.0_x64_en-US.msi.zip` | 5 MB | Windows installer bundle |
| Wayfinder `sqlite-amalgamation-*.zip` | 2.6 MB each | SQLite C amalgamation (vendor) |

### Useful docs inside `CESAROPS_FULL_BACKUP.zip`

- `cesarops-clean/TODO_RECOVERY.md` — crash recovery status (2026-04-02)
- `cesarops-clean/FILE_INVENTORY.md` — keep vs archive file list
- `cesarops-clean/FRESH_START_PLAN.md` — wipe DB + reprocess all GeoTIFFs plan
- `outputs/file_inventory.json` — machine-generated file list (~39 KB)
- `cesarops-titan-extracted/FULL_SPECTRUM_BAND_INVENTORY.md` — band/sensor inventory

Extract one file without full unpack:

```bash
unzip -p "/data/laptopdump/programming/cesarops-core/Documents/CESAROPS_FULL_BACKUP.zip" \
  "cesarops-clean/TODO_RECOVERY.md"
```

## Broken / incomplete archive

| Archive | Size | Issue |
|---------|------|--------|
| `programming/cesarops-wreckhunter build/cesarops-archive-20260402_064133.zip` | **1093 MB** | **Truncated ZIP** — local header present (`PK\x03\x04`) but **no central directory**; `unzip` / Python `zipfile` cannot open. Use the **unpacked sibling directory** `cesarops-wreckhunter build/` instead (same export, Apr 2026). |

## Relation to live repo

- Snapshot zips are **April 2026 Windows-era** flat scripts — mostly superseded by `/codebase/projects/pipelines/` (see `PIPELINE_IMPLEMENTATION_PATH.md`).
- Zips are still valuable for **recovery docs** (`TODO_RECOVERY`, band inventories) and any `.py` not copied to live.
- After backup: keep zips in tarball; do not rely on the 1.1 GB broken zip.

## Backup including archives

```bash
TS=$(date -u +%Y%m%d)
tar -czf /data/backups/laptopdump-archives-$TS.tar.gz \
  /data/laptopdump/programming/cesarops-core/Documents/*.zip \
  /data/laptopdump/Temp/nauticuvs.tar.gz
```
