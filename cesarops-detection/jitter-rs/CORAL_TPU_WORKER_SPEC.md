# Coral Edge TPU Worker — Specification (ML350e local worker)

Target: a standalone validator service running on the **ML350e** node, exposing a
small HTTP API that `jitter-rs` (on the T440) calls as a remote validator in its
cross-validation flow. Build it to this spec and it plugs in with zero changes to
the orchestrator.

## 1. Role in the pipeline

- `jitter-rs` computes a **primary candidate** (CPU/GPU) for each tile.
- It sends that candidate + the tile request to each configured validator.
- Your Coral worker runs an **independent** int8 inference on the same tile and
  returns a **vote** (agree/disagree + agreement score).
- `jitter-rs` folds all votes into a consensus certainty.

Your worker MUST be independent of the primary: do not echo the primary's
material/certainty back. Compute your own from the model output.

## 2. Hardware / runtime

- Device: Coral Edge TPU (USB `1a6e:089a`/`18d1:9302`, or PCIe `/dev/apex_0`).
- Runtime: `libedgetpu.so` (v16+), PyCoral or `tflite-runtime` 2.5.x.
- Model: a single int8 `.tflite` compiled with `edgetpu_compiler`.
- udev: install Coral rules so the worker runs as non-root (`plugdev` group).

## 3. Network

- Bind: `0.0.0.0:8190` (configurable via `CORAL_PORT`, default 8190).
- Protocol: HTTP/1.1, JSON request/response, UTF-8.
- No auth on the LAN segment; if exposed beyond the fleet VLAN, front it with a
  shared-secret header `X-Fleet-Key` (reject mismatches with 401).
- Latency budget: respond within **500 ms** p99 for a single tile. `jitter-rs`
  applies a hard client timeout (default 750 ms) and skips the validator if it
  is slow or unreachable — so timeouts degrade gracefully, never fatal.

## 4. Endpoints

### GET /health
Returns 200 with JSON:
```json
{
  "service": "coral-jitter-validator",
  "device": "coral_edgetpu",
  "tpu_present": true,
  "model_loaded": true,
  "backend": "edgetpu_int8",
  "status": "ok"
}
```
- `tpu_present`: true if a Coral device enumerates.
- `model_loaded`: true if the `.tflite` loaded onto the TPU.
- If TPU is missing or model failed to load, still return 200 with
  `status:"degraded"` and `model_loaded:false` (the worker may run a CPU
  tflite fallback; see §7).

### POST /validate
Request body (sent by jitter-rs):
```json
{
  "tile_id": "straits_t001",
  "thermal_timeseries": ["b1", "b2", "b3"],
  "coordinates": { "lat": 45.85, "lon": -84.6 },
  "depth_estimate_m": 120.0,
  "primary": {
    "material": "ferrous_composite",
    "certainty": 0.72,
    "backend": "tract_cpu"
  }
}
```

Response body (returned to jitter-rs):
```json
{
  "device": "coral_edgetpu",
  "backend": "edgetpu_int8",
  "material": "ferrous_composite",
  "certainty": 0.81,
  "agreement": 0.86,
  "agreed": true,
  "infer_ms": 12.4
}
```

## 5. Input featurization (MUST match exactly)

`jitter-rs` uses an 8-element `f32` feature vector. Your model SHOULD accept the
same features so primary and validator score comparable inputs. Build the vector
from the request fields in this exact order:

```
n      = len(thermal_timeseries)            # raw band count
index  value
  0    n
  1    min(n, 6)
  2    lat / 90.0
  3    lon / 180.0
  4    depth_estimate_m / 500.0
  5    1.0 if depth_estimate_m > 100.0 else 0.0
  6    1.0 if n >= 2 else 0.0
  7    1.0                                   # bias
```

- Model input tensor: shape `[1, 8]`, dtype int8 (quantized) — apply the model's
  input quantization (scale, zero_point) to the f32 features before inference.
- If you train a richer model that decodes the actual thermal band rasters,
  you may extend the input, but you MUST still derive a single `certainty` in
  `[0,1]` and a `material` from the output (see §6).

## 6. Output → vote mapping (MUST match jitter-rs semantics)

From the model output, produce:
- `certainty` in `[0,1]` (clamp). If the model emits 2 logits, apply softmax and
  take the probability of the "structure" class. If it emits 1 sigmoid value,
  use it directly.
- `material`: `"ferrous_composite"` if `certainty > 0.7`, else `"natural"`.
  (Use the same 0.7 threshold as the primary so votes are comparable.)

### Agreement score (MUST match this formula)

`jitter-rs` expects you to compute `agreement` identically to its internal
validators so consensus stays consistent. Given the primary candidate:

```
if material == primary.material:
    agreement = clamp(1.0 - abs(primary.certainty - certainty), 0.0, 1.0)
    agreed    = true
else:
    agreement = clamp(1.0 - certainty, 0.0, 0.5) * 0.5
    agreed    = false
```

Round `agreement` and `certainty` to 3 decimals. `agreed` is the boolean
`material == primary.material`.

### How jitter-rs uses your vote (for context, do not implement)

```
mean_agree  = mean(agreement over all validators)
agreed_frac = fraction of validators with agreed == true
if mean_agree >= 0.5:
    certainty += 0.12 * (mean_agree - 0.5) * 2 * agreed_frac
else:
    certainty -= 0.25 * (0.5 - mean_agree) * 2 * (1 - agreed_frac)
final = clamp(certainty, 0.0, 0.99)
```

So: a confident agreeing vote adds up to +0.12; a confident disagreement
subtracts up to -0.25 and can flip the tile to `natural` (certainty <= 0.7).

## 7. Failure / degradation rules

- TPU absent at startup: log a warning, set `model_loaded:false`. Either run a
  CPU `tflite-runtime` fallback (acceptable) or return 503 on `/validate`.
- Model inference error on a tile: return HTTP 200 with `agreed:false`,
  `agreement:0.0`, `certainty:0.0`, and an `"error"` string field. jitter-rs
  treats a low-agreement vote as a soft disagreement, never crashes.
- Never block longer than ~500 ms; prefer returning a degraded vote.

## 8. Model artifact

- Format: `.tflite`, fully int8-quantized, Edge TPU compiled
  (`edgetpu_compiler model_int8.tflite` → `model_int8_edgetpu.tflite`).
- Input: `[1, 8]` int8 (per §5), with quantization params baked in.
- Output: `[1, 1]` (sigmoid) or `[1, 2]` (logits) int8/float.
  **Ship v1 as sigmoid**; see `CORAL_MODEL_MIGRATION.md`. Worker auto-detects via
  `CORAL_OUTPUT=auto` (default) from the output tensor shape.
- Path via env `CORAL_MODEL` (default `/opt/cesarops/models/jitter_edgetpu.tflite`).
- Override only if needed: `CORAL_OUTPUT=sigmoid|logits|auto`.
- Until a trained model exists, ship a stub that computes `certainty` from the
  §5 features with a fixed rule (e.g. `0.6 + 0.06*min(n,6)`), so the HTTP
  contract and consensus wiring can be validated before the real model lands.

## 9. Process / deployment

- Run as a foreground process (systemd or Nomad), restart on failure.
- Env:
  - `CORAL_PORT` (default 8190)
  - `CORAL_MODEL` (path to `.tflite`)
  - `FLEET_KEY` (optional shared secret for `X-Fleet-Key`)
- Logs: structured line logs to stdout; include `tile_id` and `infer_ms`.
- Expose on the fleet VLAN so the T440 can reach
  `http://<ml350e-ip>:8190/validate`.

## 10. Acceptance tests

1. `GET /health` → 200, `tpu_present:true`, `model_loaded:true`.
2. `POST /validate` with the §4 example → 200 with all required fields, and
   `agreement`/`agreed` consistent with §6 given the included `primary`.
3. Disagreement case: send `primary.material:"natural"` with a high-band tile;
   confirm `agreed:false` and `agreement` computed by the disagreement branch.
4. Latency: 100 sequential `/validate` calls, p99 < 500 ms.
5. Kill the TPU (unplug / stop): `/health` reports `degraded`, `/validate` still
   returns a well-formed degraded vote (no 5xx storm).

## 11. Reference: the T440 side

`jitter-rs` reaches you via its `remote` validator (see `src/validators/remote.rs`),
configured with `JITTER_REMOTE_VALIDATORS=coral_edgetpu=http://<ml350e-ip>:8190`.
It POSTs to `/validate`, applies a 750 ms timeout, and records your returned vote
verbatim. If you are unreachable, the validator is silently skipped.
