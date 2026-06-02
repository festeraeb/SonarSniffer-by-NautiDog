# nautik9 (WSL) — NFS mount + cluster join

## Confirmed working (no NFS required)

| Check | Expected |
|-------|----------|
| Nomad | `100.110.214.86:4646` — **ready** |
| Consul | `100.110.214.86:8301` — **alive** |
| WSL route | `100.110.214.86` (TS) / `10.0.0.69` (LAN) — not `172.27.x` |

Consul: use a **drop-in** `/etc/consul.d/10-cesarops-advertise.hcl` so it wins over stale `consul.hcl` `172.27` lines.

## c2 vs laptop

cesarops2 mounts the same export from `10.0.0.201` → `10.0.0.61:/codebase/repos/wreckhunter2000-1` (NFSv4). **ACL is fine.**

WSL2 often returns **`Operation not permitted`** even when port 2049 is open — client limitation, not T440 `exports`.

## Mount order to try (WSL)

### A — NFSv4 parent (recommended after T440 `fsid=0` on `/codebase`)

```bash
T440=100.72.182.77   # off-site; use 10.0.0.61 at home
sudo apt-get install -y nfs-common
sudo mkdir -p /mnt/t440-codebase
sudo mount -t nfs4 ${T440}:/codebase /mnt/t440-codebase \
  -o rw,hard,timeo=600,retrans=2,_netdev,vers=4.2
sudo mkdir -p /codebase/repos/wreckhunter2000-1
sudo mount --bind /mnt/t440-codebase/repos/wreckhunter2000-1 /codebase/repos/wreckhunter2000-1
findmnt /codebase/repos/wreckhunter2000-1
```

### B — NFSv3 (LAN only)

```bash
sudo mount -t nfs -o vers=3,nolock 10.0.0.61:/codebase/repos/wreckhunter2000-1 \
  /codebase/repos/wreckhunter2000-1
```

### C — No NFS (bootstrap only)

If mounts never work, run from a **git-synced** tree (not `C:\Users\...\programming` unless pulled):

```bash
cd ~/codebase-src/wreckhunter2000-1   # must contain infra/nomad/
export CESAROPS_ALLOW_LOCAL_REPO=1
export CESAROPS_CONSUL_ADVERTISE=100.110.214.86
export CESAROPS_T440_TS_IP=100.72.182.77
sudo -E bash infra/nomad/scripts/bootstrap_client.sh nautik9-laptop m2200 --remote
```

Nomad/Consul already correct — bootstrap is only needed to refresh unit configs.

## T440 server-side (from cesarops2)

Re-apply exports with NFSv4 `fsid=0` on `/codebase`:

```bash
NOMAD_ADDR=http://10.0.0.61:4646 nomad job run infra/nomad/jobs/t440-nfs-exports.nomad.hcl
```

During a laptop mount attempt, on T440:

```bash
sudo journalctl -u nfs-server -f
```

## require_mounted_repo

Passes only on **nfs/nfs4** mounts. Bind-mount from `/mnt/t440-codebase/...` is OK if underlying fs is nfs4.

Set `CESAROPS_ALLOW_LOCAL_REPO=1` only for emergency bootstrap from `~/codebase-src` (not for fleet/script cleanup).
