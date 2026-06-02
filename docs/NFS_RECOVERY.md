# NFS recovery — T440 ↔ cesarops2

When **T440** (`10.0.0.61`) needs filesystem or service repair, use **cesarops2** (`10.0.0.201`) with NFS so you edit the same paths—no stale copies.

## T440 — enable exports (once)

```bash
cd /codebase/repos/wreckhunter2000-1
sudo bash scripts/setup_t440_nfs_exports.sh
```

Exports to LAN `10.0.0.0/24` and Tailscale `100.64.0.0/10` (laptop off-site):

```bash
# Default adds both subnets; TS-only patch: CESAROPS_LAN= CESAROPS_TS_SUBNET=100.64.0.0/10
sudo bash scripts/setup_t440_nfs_exports.sh
```

| Export | Purpose |
|--------|---------|
| `/data` | PERC RAID10 data partition |
| `/codebase` | PERC RAID10 codebase tree: `projects/`, pipelines, mag grids |
| `/codebase/repos/wreckhunter2000-1` | Full repo + forge |
| `/mnt/raid0` | PERC RAID0 models + downloads store |
| `/home/cesarops` | Configs, scripts, SSH keys |

### nautik9 (laptop, WSL)

- **At home:** mount `10.0.0.61:/codebase/repos/wreckhunter2000-1` (WSL mirrored networking → `10.0.0.x` source).
- **Off-site:** mount `100.72.182.77:/codebase/repos/wreckhunter2000-1` after Tailscale export ACL exists.
- **Do not** use `~/codebase-src` or `C:\Users\...\programming\...` for Nomad/Consul — use mounted `/codebase/repos/wreckhunter2000-1` only.
- Consul advertise: `100.110.214.86` (Windows TS) if Tailscale is not in WSL.

## cesarops2 — mount everything

```bash
T440_IP=10.0.0.61 bash /mnt/t440/repo/scripts/setup_cesarops2_mag_data.sh
# or after first mount:
source ~/.config/cesarops/mag_paths.env
```

Canonical mount tree on cesarops2:

```
/mnt/t440/data              ← T440:/data
/mnt/t440/codebase          ← T440:/codebase (projects + pipelines)
/mnt/t440/repo              ← T440 repo
/mnt/t440/models            ← T440 RAID0 models + downloads volume
/mnt/t440/home              ← T440 /home/cesarops
```

**Aeromag work:** `/mnt/t440/codebase/projects/pipelines/magnetic_data`  
(~824M raw USGS + grids; older `adaptive_bg_*` dirs must be regenerated if missing)

## Health / repair workflow

1. Stop forge on T440 if disk repair needed: `sudo systemctl stop cesarops-forge-v2`
2. From cesarops2, confirm mounts: `mount | grep 10.0.0.61`
3. Edit or rsync under `/mnt/t440/...`
4. Remount on T440 after fix: `sudo exportfs -ra`
5. Restart services on T440

## IPs

| Host | LAN IP |
|------|--------|
| T440 (this machine) | 10.0.0.61 |
| cesarops2 | **10.0.0.201** |
| cesarops3 (scout) | 10.0.0.41 |
