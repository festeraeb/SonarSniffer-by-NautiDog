# DEPRECATED — cesarops2-fr was a misread

**Use the ML350e as `cesarops2` only.** See `scripts/setup_cesarops2_ml350e.sh`.

---

# cesarops2-fr (obsolete note)

~~New host **cesarops2-fr** on Tailscale~~ — not used. The ML350e at **`10.0.0.201`** is **`cesarops2`** (`100.102.158.111` on Tailscale when joined).

## On cesarops2-fr (first boot)

```bash
# Clone or NFS-mount repo, then:
cd /codebase/repos/wreckhunter2000-1   # or /mnt/t440/codebase/repos/wreckhunter2000-1

# Join Tailscale (hostname cesarops2-fr)
export TS_AUTHKEY="tskey-auth-..."   # from Tailscale admin
sudo bash scripts/setup_cesarops2_fr_tailscale.sh

tailscale ip -4    # note IPv4 for Forge patch
```

Configs installed to `/etc/cesarops/`:
- `cesarops-node.toml` — same roles as `cesarops-node-cesarops2.toml`
- `cesarops2-fr.env` — `CESAROPS2_HOST`, Forge URL, T440 Tailscale IP

## From T440 (push + patch Forge)

```bash
REMOTE=cesarops@cesarops2-fr bash scripts/deploy_configs_to_cesarops2_fr.sh
# or: REMOTE=cesarops@<FR-tailscale-ip> bash ...
```

## Ports (unchanged from cesarops2)

| Port | Service |
|------|---------|
| 5200 | Thinker / RTX 2060 |
| 5571 | Draft / GTX 1070 |
| 10129–10130 | Cake hetero (2060 + 1070; P106 when not on PCIe x1) |

## Cake / fleet

```bash
source /etc/cesarops/cesarops2-fr.env
export CESAROPS2_HOST   # set to FR Tailscale IP for T440→c2 over mesh
bash scripts/cake/prep-c2-hetero-workers.sh
```

Legacy LAN `10.0.0.201` remains in `cluster_config` for the ML350e until retired.
