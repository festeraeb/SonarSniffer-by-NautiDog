# Fleet Wiring Playbook (T440 + cesarops2)

Use this to wire the full stack: LLM endpoints, Forge routing, n8n workflows, queue runner, and health/recovery loops.

## 1) Start model endpoints

On T440:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/p100_gemma_r1_dual.sh start
bash /codebase/repos/wreckhunter2000-1/scripts/p100_gemma_r1_dual.sh status
```

On cesarops2:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/cesarops2_qwen36_gemma4_dual.sh start
bash /codebase/repos/wreckhunter2000-1/scripts/cesarops2_qwen36_gemma4_dual.sh status
```

Expected primary endpoints:
- T440 coder: http://127.0.0.1:5001
- T440 reviewer: http://127.0.0.1:5002
- cesarops2 thinker: http://10.0.0.201:5200
- cesarops2 gemma: http://10.0.0.201:5571

## 2) Apply Forge routing preset

On T440:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/forge-routing-switch.sh preset p100-gemma-r1
bash /codebase/repos/wreckhunter2000-1/scripts/forge-routing-switch.sh status
```

If you want edge profile instead:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/forge-routing-switch.sh edge
```

## 3) Activate n8n workflows + orchestration

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/prep_n8n_fleet_live.sh
bash /codebase/repos/wreckhunter2000-1/scripts/n8n_activate_fleet_workflows.sh
```

This imports/activates fleet and PAMP workflows, then enables live orchestration mode in Forge.

## 4) Wire remote queue processing (cesarops2)

Run once on cesarops2 to mount T440 repo and enable queue recovery timers:

```bash
T440_IP=10.0.0.61 bash /codebase/repos/wreckhunter2000-1/scripts/setup_cesarops2_fleet_recovery.sh
```

This enables fleet jobs through shared queue paths under:
- /mnt/t440/repo/var/fleet-jobs/pending/t440
- /mnt/t440/repo/var/fleet-jobs/pending/cesarops2

## 5) Validate route health end to end

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/fleet-route-health.sh
bash /codebase/repos/wreckhunter2000-1/scripts/fleet-sync-cesarops2-llm.sh
```

If unhealthy, recover with:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/forge-health-recover.sh interrupt_and_clear
bash /codebase/repos/wreckhunter2000-1/scripts/forge-health-recover.sh restart_forge
bash /codebase/repos/wreckhunter2000-1/scripts/forge-health-recover.sh restart_llama_p100
```

## 6) Dispatch a smoke fleet action

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/fleet-n8n-dispatch.sh cesarops2 sync_llm_endpoints
bash /codebase/repos/wreckhunter2000-1/scripts/fleet-n8n-dispatch.sh t440 route_health_check
```

## 7) Deep scan execution mode

For resilient deep dispatch with retries and timeout guard:

```bash
nohup env RESET_DEEP=1 DISPATCH_MAX_SECS=5400 \
  /codebase/repos/wreckhunter2000-1/scripts/blueprint_audit_deep_watchdog.sh \
  > /codebase/repos/wreckhunter2000-1/var/log/blueprint_audit_deep_watchdog.nohup.log 2>&1 &
```

Watch progress:

```bash
tail -n 120 /codebase/repos/wreckhunter2000-1/var/log/blueprint_audit_deep_watchdog.nohup.log
python3 - <<'PY'
import json, pathlib
p=pathlib.Path('/codebase/repos/wreckhunter2000-1/reports/blueprint_audit/llm_results/fleet_dispatch_manifest.json')
if p.exists():
    m=json.loads(p.read_text())
    print(m.get('status'), m.get('summary'))
    for b in sorted(m.get('batches',[]), key=lambda x:x.get('batch_id','')):
        print(b.get('batch_id'), b.get('status'), b.get('processed_size'))
else:
    print('manifest missing')
PY
```

## 8) Quick wiring checklist

- 5001 and 5002 return /v1/models on T440.
- 5200 and 5571 return /v1/models on cesarops2.
- Forge /health is up on :9100.
- n8n /healthz is up on :5678.
- fleet-route-health passes all required checks.
- Fleet webhook dispatch reaches both nodes.
- Deep watchdog running with timeout and retry loop.

## 9) Dynamic compute discovery and self-healing mission services

Run one watchdog tick manually:

```bash
bash /codebase/repos/wreckhunter2000-1/scripts/mission_service_watchdog.sh
```

This watchdog will:
- Verify n8n and Forge health and restart if needed
- Discover compute from Forge and NautiInferer registries
- Auto-apply available thinker/reviewer/corrector routing
- Attempt recovery via local LLM and CPU worker fallbacks when pools are empty

Inspect discovered compute map:

```bash
python3 /codebase/repos/wreckhunter2000-1/scripts/discover_compute_sources.py \
  --forge-url http://127.0.0.1:9100 \
  --nauti-url http://127.0.0.1:8099
```

Add newly acquired nodes without code changes:

```bash
export EXTRA_LLM_ENDPOINTS="thinker=http://10.0.0.202:6200,reviewer=http://10.0.0.203:6201"
bash /codebase/repos/wreckhunter2000-1/scripts/mission_service_watchdog.sh
```

Fleet action name (for queue/n8n):
- `mission_service_watchdog`
