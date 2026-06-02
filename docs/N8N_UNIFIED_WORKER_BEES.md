# n8n Unified Worker Bees

This pattern gives you one pipeline contract and expert worker bees that only tune variables.

## Core idea

- Keep one mission schema and orchestration flow.
- Use profile presets per mission type (water wreck, vessel SAR, land aircraft SAR, land vehicle SAR, person search, public sonar scan).
- Let n8n workers specialize by lane but write to the same candidate contract.

## Files

- Profile packs: config/unified_pipeline_profiles.json
- Mission template: missions/dual_use_mission_template.json
- Resolver worker: scripts/unified_pipeline_worker_bee.py
- Generated missions: missions/generated/*.json

## Worker bee roles

1. Planner bee
- Converts user intent into `pipeline_type` + high-level overrides.

2. Weather bee
- Sets `weather_filter` and weather-related knobs by domain and urgency.

3. Download bee
- Tunes sensors, date windows, and max download limits.

4. Slice bee
- Tunes chip size/overlap and temporal windows.

5. Analysis bees
- Satellite bee, magnetic bee, drift bee, bag bee, PDF bee, sonar bee.
- They only change lane knobs and enable flags.

6. Fusion bee
- Applies mission confidence thresholds and ranking profile.

7. Report bee
- Produces admin/field package outputs for SAR operations.

## n8n payload shape for resolver

```json
{
  "pipeline_type": "land_aircraft_sar",
  "mission_id": "ALASKA_CRASH_2026_05_10",
  "target_name": "Overdue fixed-wing aircraft",
  "bbox": [61.2, -150.5, 62.5, -149.0],
  "date_range": ["2026-05-07", "2026-05-15"],
  "overrides": {
    "pipeline": {
      "run_bag": false,
      "run_sonar": false
    },
    "knobs": {
      "fusion_high_threshold": 0.84
    }
  }
}
```

## Resolver command examples

Resolve only:

```bash
python3 scripts/unified_pipeline_worker_bee.py --payload-file /tmp/mission_payload.json
```

Resolve and execute mission control:

```bash
python3 scripts/unified_pipeline_worker_bee.py --payload-file /tmp/mission_payload.json --execute
```

Inline quick run:

```bash
python3 scripts/unified_pipeline_worker_bee.py \
  --pipeline-type water_wreck_hunt \
  --mission-id STRAITS_SWEEP_001 \
  --bbox 45.6,-84.9,45.9,-84.4 \
  --date-start 2026-05-01 \
  --date-end 2026-05-20
```

## Why this matches your goal

- End user hits one LLM and one mission entry point.
- n8n worker bees act as expert tool turners by setting profile variables.
- Logic stays centralized; only mission knobs and lane toggles vary.
- Same architecture serves water-first operations and land SAR adaptation.

## Dynamic compute expansion (NautiInferer network)

When you add new hardware, avoid rewriting n8n logic. Keep workers on the same pipeline and let compute discovery feed routing.

Discovery and routing scripts:
- scripts/discover_compute_sources.py
- scripts/mission_service_watchdog.sh

Recommended pre-mission tick in n8n:
1. Execute `mission_service_watchdog` fleet action before mission dispatch.
2. Use generated routing from watchdog for thinker/reviewer/corrector lanes.
3. If zero healthy GPU endpoints, watchdog attempts local CPU/worker fallback automatically.

Optional endpoint injection for fresh nodes:

```bash
EXTRA_LLM_ENDPOINTS="thinker=http://10.0.0.202:6200,reviewer=http://10.0.0.203:6201" \
bash /codebase/repos/wreckhunter2000-1/scripts/mission_service_watchdog.sh
```
