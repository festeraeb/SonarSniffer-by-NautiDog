# Script cleanup — Forge mission + n8n hygiene

**Goal:** Inventory `.sh` sprawl on T440 + cesarops2 (shared NFS repo), classify keep vs archive vs delete, tune Forge/agents on the audit, then automate removal of approved one-offs.

**Status:** Planned — run as next Forge `/send` mission after cluster migration stabilizes.

---

## Why

Migration and GPU routing added many **one-shot** scripts (`t440-*`, `cluster-exec-*`, dated downloads, Cake install attempts). The repo has **~150+ scripts under `scripts/`** and duplicates (e.g. `fleet-job-runner.sh` at repo root). Shared NFS means **one cleanup benefits both nodes**.

---

## Phase 1 — Inventory (no deletes)

On **either** node (same repo tree):

```bash
cd /codebase/repos/wreckhunter2000-1
bash scripts/scan-script-inventory.sh --out var/script-inventory/latest
```

Deliverables:

- `var/script-inventory/latest/manifest.json` — machine-readable
- `var/script-inventory/latest/report.md` — human review
- `var/script-inventory/latest/by-tier/{keep,review,ephemeral}.txt`

**T440 vs c2:** Only list **extra** scripts outside the repo (systemd drop-ins, `~/bin`, `/tmp`) if present:

```bash
# Optional local-only paths (not in git)
ls -la ~/bin/*.sh 2>/dev/null
ls /etc/systemd/system/cesarops*.service 2>/dev/null
```

---

## Phase 2 — Forge mission (classify + propose moves)

Dispatch to **primary Forge** (`http://10.0.0.201:9100`):

**Task title:** `script-inventory-cleanup-202606`

**Prompt sketch:**

> Read `var/script-inventory/latest/report.md` and `manifest.json`. For each script:
> 1. **keep** — production (watchdogs, fleet, forge routing, rclone, nomad bootstrap, webpage/dashboard updaters, build_sonarsniffer_*, start_n8n, credentials).
> 2. **archive** — useful reference; move to `scripts/archive/YYYY-MM/` with one-line README.
> 3. **ephemeral** — safe to delete after review; must add header `# CESAROPS_EPHEMERAL=1` or move to `scripts/_ephemeral/`.
>
> Output: `var/script-inventory/latest/forge-verdict.json` with `{path, tier, reason, referenced_by[]}`.
> Do not delete files in Phase 2 — only write verdict.
> Flag duplicates (same purpose, different names) and superseded pairs (e.g. `t440-disable-deprecated-forge.sh` vs `t440-remove-forge.sh`).

Use dual-coder preset; reviewer checks for false positives on fleet/forge-health scripts.

---

## Phase 3 — Human gate

Review `forge-verdict.json`. Minimum bar before any delete:

- [ ] No `keep` script marked ephemeral
- [ ] Webpage/dashboard scripts explicitly **keep**
- [ ] `infra/nomad/**` and `systemd/cesarops-*.service` untouched

---

## Phase 4 — n8n auto-cleanup (hourly dry-run, daily apply)

Workflow: **`Script Ephemeral Cleanup`** (create in n8n)

| Step | Action |
|------|--------|
| Cron | `0 * * * *` dry-run; `0 4 * * *` apply (adjust) |
| Execute | `bash scripts/cleanup-ephemeral-scripts.sh` |
| Env | `DRY_RUN=1` (hourly) / `DRY_RUN=0` (daily) |
| Notify | Slack/email on non-zero deletes |

**Safety rules (enforced in script):**

- Only deletes under `scripts/_ephemeral/` OR files with `# CESAROPS_EPHEMERAL=1` in first 5 lines
- Never deletes `scripts/lib/`, `infra/`, `systemd/`, `cesarops-forge-v2/scripts/`
- Requires `var/script-inventory/latest/forge-verdict.json` younger than 7 days
- Logs to `var/log/script-cleanup.log`

Fleet enqueue (alternative):

```bash
FLEET_DISPATCH=nfs bash scripts/fleet-n8n-dispatch.sh cesarops2 script_cleanup_ephemeral \
  dry_run=1
```

(Add `script_cleanup_ephemeral` action to `fleet-job-runner.sh` when workflow is live.)

---

## Known tiers (seed list for Forge)

### Keep (do not auto-delete)

| Area | Examples |
|------|----------|
| Fleet / Nomad | `fleet-*`, `cluster-exec-t440.sh`, `cluster-verify-rclone.sh`, `setup_t440_nfs_exports.sh`, `rclone-setup-t440.sh` |
| Forge | `forge_apply_*`, `forge-health-*`, `forge-sync-state.sh`, `forge-routing-switch.sh`, `deploy_forge.sh` |
| Watchdogs | `n8n-watchdog.sh`, `mission_service_watchdog.sh`, `ensure_n8n_watchdog.sh` |
| n8n / web | `start_n8n.sh`, `safe-n8n-run.sh`, `import_n8n_*`, `launch_t440_dashboard.sh` |
| Ops | `setup_cesarops_tunnel.sh`, `credentials.sh`, `cesarops-fleet-mode.sh` |
| Build | `build_sonarsniffer_*.sh` |

### Review (likely archive, not delete)

| Examples | Note |
|----------|------|
| `t440-disable-deprecated-forge.sh` | Superseded by `t440-remove-forge.sh` |
| `download_model_candidates_20260531.sh` | Dated one-off |
| `cesarops2_fr_*`, `deploy_configs_to_cesarops2_fr.sh` | Only if FR node unused |
| `cake/*` install/prep/fix scripts | Keep `start-fleet-cluster.sh`, `stop-fleet-cluster.sh` |
| `forge_run_until_green.sh`, `forge_overnight_watch.sh` | Overlap with `forge-health-probe` |

### Ephemeral candidates (after Forge verdict)

| Examples | Note |
|----------|------|
| `fleet-job-runner.sh` (repo root) | Duplicate of `scripts/fleet-job-runner.sh` |
| Ad-hoc `integrate/run_*_dispatch.sh` | If Forge dispatch replaced |

---

## Success criteria

1. `manifest.json` generated on both nodes (same NFS path).
2. Forge produces `forge-verdict.json` with &lt;5% disputed paths.
3. n8n hourly dry-run runs 7 days with zero accidental targets.
4. Daily apply reduces `scripts/` count without breaking fleet timers or Forge health.

---

## Related

- [MIGRATION_STATUS.md](./MIGRATION_STATUS.md)
- [FLEET_WIRING_PLAYBOOK.md](./FLEET_WIRING_PLAYBOOK.md) — blueprint audit pattern
- `scripts/fleet-clean-junk.sh` — dispatch artifacts only (not `.sh` inventory)
