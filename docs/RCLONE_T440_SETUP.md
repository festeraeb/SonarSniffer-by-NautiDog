# rclone on T440 — fix config + Nomad periodic job

## 1. Fix `found key without value`

That error means `~/.config/rclone/rclone.conf` has a broken line (e.g. `type =` with nothing after `=`).

On **T440**:

```bash
# Remove or backup broken file, then install a valid stub
mkdir -p ~/.config/rclone
mv ~/.config/rclone/rclone.conf ~/.config/rclone/rclone.conf.broken.$(date -u +%Y%m%dT%H%M%SZ) 2>/dev/null || true

bash /codebase/repos/wreckhunter2000-1/scripts/rclone-setup-t440.sh
```

Or manually:

```bash
cp /codebase/repos/wreckhunter2000-1/infra/nomad/rclone/rclone.conf.example ~/.config/rclone/rclone.conf
chmod 600 ~/.config/rclone/rclone.conf
mkdir -p /codebase/backups/cesarops-state
rclone listremotes
# expect: cesarops-backup:
```

## 2. Test sync (not SKIP)

```bash
bash /codebase/repos/wreckhunter2000-1/infra/nomad/rclone/rclone-state-sync.sh
# expect: rclone state sync complete
ls -la /codebase/backups/cesarops-state/state/
```

Staging remote is **local alias** → `/codebase/backups/cesarops-state`. Swap the `[cesarops-backup]` block in `rclone.conf` for S3 when you have cloud credentials.

## 3. Nomad periodic job — correct log commands

Parent job `rclone-state-sync` has **no allocations** until the scheduler fires a child. Use:

```bash
export NOMAD_ADDR=http://127.0.0.1:4646

nomad job run /codebase/repos/wreckhunter2000-1/infra/nomad/jobs/rclone-state-sync.nomad.hcl

nomad job periodic status rclone-state-sync
nomad job status rclone-state-sync

# After next launch (or force dispatch):
nomad job periodic force rclone-state-sync

# List child batch allocations
nomad job allocs -all rclone-state-sync

# Logs (use alloc ID from above, or latest child job id)
ALLOC=$(nomad job allocs -all rclone-state-sync -json | python3 -c "import sys,json; a=json.load(sys.stdin); print(a[0]['ID'] if a else '')")
nomad alloc logs "$ALLOC"
nomad alloc logs -stderr "$ALLOC"
```

`nomad alloc logs -job rclone-state-sync` often fails on periodic parents — use **alloc ID** or **child job ID** `rclone-state-sync/periodic-<unix>`.

## 4. Cloud remote later

Edit `~/.config/rclone/rclone.conf` — see commented S3 block in `rclone.conf.example`. After change:

```bash
rclone listremotes
bash /codebase/repos/wreckhunter2000-1/infra/nomad/rclone/rclone-state-sync.sh
```
