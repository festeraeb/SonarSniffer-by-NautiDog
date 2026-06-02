# n8n CESAROPS Tuning (Coding + Ops + DAS)

## What was tuned
- Fleet Ops Dispatch: action allowlist, mode support (`coding` or `ops`), DAS flag (`use_das`), backend hint (`sync_backend`: `samba` or `nfs`).
- MoE Tool Router: command execution now goes through safe policy wrapper.
- Prompt Tuner Worker: profile + DAS context fields for coding/ops pipelines.

## Files
- Exported active workflows: `n8n/exports/*.json`
- Tuned workflows ready to import: `n8n/tuned/*.json`
- Safe command policy script: `scripts/safe-n8n-run.sh`
- Sync script (local X: -> both forges): `n8n/sync-to-forges.ps1`

## Import into n8n (UI)
1. Open n8n UI.
2. Import each file from `n8n/tuned/`.
3. Disable old workflows after validation.
4. Activate tuned workflows.

## Suggested webhook payload (Fleet Ops tuned)
```json
{
  "node": "cesarops2",
  "action": "build_release",
  "mode": "coding",
  "use_das": true,
  "sync_backend": "samba",
  "repo": "/codebase/repos/wreckhunter2000-1"
}
```

## Sync from this machine
```powershell
powershell -ExecutionPolicy Bypass -File X:\cesarops\repo\n8n\sync-to-forges.ps1
```
