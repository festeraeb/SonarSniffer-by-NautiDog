# Satellite glint / pattern-lock scan → fleet coders

## What TPU + Movidius are for (yes — pattern matching)

| Signal | Accelerator | Role |
|--------|-------------|------|
| **Glint** (specular peaks, same pixel neighborhood) | Coral TPU `:8092` `/infer` | Fast dense scan — bright local maxima, repeat visits |
| **Thermal jitter** (heat sink vs natural) | Movidius T440 `:8180` `/jitter` | Temporal signature at fixed lat/lon |
| **Fixed-site cues** | Both + downstream LLMs | Same tile: glint + dark patch + clearer water + ripple phase |

Not full science review — **spatial consistency** and **material class** at anchor points. P100s/RTX/1070 turn the accel packet into **candidate hull geometry + QA checklist** for sunken-ship search.

## Targets (wreck hunter)

Per tile / repeat pass, flag anchors where **multiple** of:

1. Glint above percentile (TPU detections)
2. Jitter says non-natural / metal-likely (Movidius)
3. **Dark spot** persistent (negative Δ reflectance vs neighborhood)
4. **Clearer water** persistent (low turbidity vs neighborhood)
5. **Ripple/glint** same sub-pixel phase across passes

## Phase A — Accel scan (direct HTTP, not Forge)

1. `POST http://10.0.0.201:8092/infer` — `{ "image_base64", "meta": { "tile_id", "lat", "lon", "pass_id" } }`
2. `POST http://10.0.0.61:8180/jitter` — `{ "tile_id", "thermal_timeseries", "coordinates", "depth_estimate_m" }`
3. Fuse into **ACCEL_SCAN_PACKET** (JSON): detections + jitter signature + anchor hypotheses

## Phase B — Coder task (same packet → each LLM)

Endpoints (P106 **excluded**):

| Node | Port | Model lane |
|------|------|------------|
| T440 P100 #1 | 5001 | Gemma |
| T440 P100 #2 | 5002 | Qwen |
| c2 RTX 2060 | 5200 | (loaded model) |
| c2 GTX 1070 | 5202 or 5203 | (loaded model) |

**Prompt:** Given ACCEL_SCAN_PACKET, produce:

- Ranked **wreck candidates** (lat/lon anchors)
- Which cues fired (glint/dark/clear/jitter)
- **False-positive guards** (sandbar, boat wake, cloud)
- Minimal **Python** sketch: `process_tile(packet) -> candidates[]` paths under `cesarops-detection/`

## Phase C — Grade (human)

Score 0–100: anchor logic, use of accel fields, FP guards, code paths, brevity.
