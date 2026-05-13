# CESAROPS Cluster Operations — Things Workers Must Know

This is the operational knowledge that gets injected into worker agents via the
Tool Recipe Database. These are hard-learned lessons about how the cluster actually
behaves — not how it should behave in theory.

## Killing Processes on T440

systemd services with `Restart=always` will respawn within 10 seconds of being killed.
You MUST stop the service unit FIRST, then kill the process:

```bash
# WRONG — process comes back in 10s:
kill <pid>

# RIGHT — stop the unit, THEN it stays dead:
sudo systemctl stop koboldcpp
# OR disable it entirely:
sudo systemctl disable --now koboldcpp
```

If you don't have sudo, you need to update the service file and reload:
```bash
sudo cp /path/to/new.service /etc/systemd/system/koboldcpp.service
sudo systemctl daemon-reload
sudo systemctl restart koboldcpp
```

## KoboldCPP Model Swaps

The P100s can only hold ONE model at a time (32GB HBM2 total).
To swap models:
1. Stop the systemd service (not just kill the process)
2. Wait 3 seconds for GPU memory to free
3. Start with new model — if CUDA OOM, the old process is still holding VRAM

## Tailscale SSH

All nodes are on Tailscale. Use Tailscale IPs, not LAN IPs:
- T440: 100.72.182.77 (cesarops@)
- cesarops2: 100.102.158.111
- cesarops3: 100.105.77.74
- Pi: 100.127.66.32

SSH keys are pre-authorized. No passwords needed for `cesarops` user.
But `sudo` requires a password on T440 (not passwordless).

## Port Assignments (T440)

- 5001: KoboldCPP (LLM inference)
- 8099: wrecks-api (FastAPI/uvicorn)
- 9000: supervisor node (sovereign-cloud TCP)

## Service Files Location

- `/etc/systemd/system/koboldcpp.service` — LLM inference
- `/etc/systemd/system/wrecks-api.service` — REST API
- Source copies: `scripts/*.service` in the repo

## GPU Memory Layout

Dual P100 16GB each = 32GB total HBM2.
- Qwen3.6-35B MXFP4: ~20GB (fits in both P100s with --gpulayers 64)
- Qwen2.5-Coder-14B Q4_K_M: ~8GB (fits in one P100 with room for draft model)
- When switching models: MUST fully stop old process, wait for VRAM release

## File Paths on T440

- Models: `/mnt/data-external/cesarops/models/`
- Repo: `/home/cesarops/wreckhunter2000-1/`
- API venv: `/home/cesarops/api-venv/`
- KoboldCPP binary: `/home/cesarops/koboldcpp`
- Research log: `/home/cesarops/wreckhunter2000-1/research_log/`
- Scan outputs: `/home/cesarops/wreckhunter2000-1/outputs/`

## Common Failure Modes

1. **CUDA OOM on model swap** — old process still holding VRAM. Fix: `sudo systemctl stop koboldcpp` first.
2. **API 500 on erie/datum endpoints** — missing Python modules (not deployed). Returns graceful JSON error now.
3. **Research daemon dies silently** — Python stdout buffering. Check the JSON log file, not stdout.
4. **systemd respawns old config** — service file on disk wasn't updated. Must `sudo cp` + `daemon-reload`.
5. **scp transfer incomplete** — Tailscale can drop long transfers. Verify file size after transfer.

## Worker Agent Rules

When dispatching tasks to worker nodes:
1. Always check if the target service is running BEFORE sending work
2. If a service needs restarting, use systemctl — never raw kill
3. Verify GPU memory is free before loading a model (nvidia-smi)
4. Log every model swap with timestamp — helps debug OOM cascades
5. The research daemon and KoboldCPP share the P100s — never run both simultaneously
