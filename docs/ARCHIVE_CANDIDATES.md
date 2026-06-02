# Archive candidates (unify → backup → archive)

Use after `docs/PIPELINE_IMPLEMENTATION_PATH.md` port/merge steps.

## In-repo directories (safe to move off active tree)

| Path | Action |
|------|--------|
| `backup/` | Move to `/data/backups/wreckhunter2000-1-inrepo-backup-$TS/` after tarball; optional read-only symlink back |
| `backup/deploy/tools/pipelines/` | Duplicate of `/codebase/projects/pipelines` — archive first |
| `backup/wreckhunter2000-1/` | Old snapshot — do not build from here |
| `cesarops-forge-v2/dispatch_results/` | LLM JSON dumps — archive or add to `.gitignore` |
| `.cargo-docker/` | Registry cache — exclude from git backups |
| `target/` | Build artifacts — never commit |

## External paths (tarball first, do not delete)

| Path | Action |
|------|--------|
| `/codebase/repos/laptop-code/` | `tar czf` → `/data/backups/`, then read-only |
| `/data/laptopdump/` | Keep until pipeline port verified |
| `/mnt/data-external/repos/laptop-code/` | Duplicate bundle — archive if identical to laptop-code |
| `/codebase/wreckhunter2000-1` | Legacy layout — confirm unused, then archive |

## Root-level Python (live repo — review, not bulk archive)

Files at repo root (`universal_downloader.py`, `lake_michigan_scan.py`, etc.) should be compared to `pipelines/` and either moved under `pipelines/` or linked from Forge — avoid duplicating logic in two places.

## Git hygiene before push

1. `.gitignore`: `target/`, `*.log`, `dispatch_results/*.json`, `.env`
2. Commit: `docs/PIPELINE_IMPLEMENTATION_PATH.md`, `docs/ARCHIVE_CANDIDATES.md`, active crates only
3. Do not commit `backup/` if removing from tree (use archive branch or tarball only)

## Suggested commands (after backup verified)

```bash
TS=$(date -u +%Y%m%d)
sudo mkdir -p /data/backups

# Full backups
tar -czf /data/backups/pipelines-live-$TS.tar.gz -C /codebase/projects pipelines
tar -czf /data/backups/laptop-code-$TS.tar.gz -C /codebase/repos laptop-code
tar -czf /data/backups/wreckhunter2000-1-$TS.tar.gz \
  -C /codebase/repos wreckhunter2000-1 \
  --exclude=wreckhunter2000-1/target \
  --exclude=wreckhunter2000-1/.cargo-docker

# Move in-repo backup tree off active path
mv /codebase/repos/wreckhunter2000-1/backup \
  /data/backups/wreckhunter2000-1-inrepo-backup-$TS
```
