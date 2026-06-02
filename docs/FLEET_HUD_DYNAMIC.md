# Fleet HUD — dynamic roles and fleet-wide temps

## Corner HUD (`forge-hud.js`)

| Source | Shows |
|--------|--------|
| **`GET /gpu/stream` (SSE, ~2s)** | **Live NVML** — same data family as **nvtop** (util, mem%, power, processes, sparkline in HUD) |
| `GET /monitor` | Snapshot fallback — T440 `nvidia-smi` + node heartbeats; SSH tunnel to cesarops2 if heartbeat missing |
| `GET /validate/ping` | **Named LLM roles** (poll ~45s so bench does not load GPUs) |

The ncurses **nvtop** UI cannot be embedded in the browser; Forge streams its metrics over SSE instead.

### GPU rows

Each row: **`host` + GPU name + index**, **temperature °C**, **util %**, **effective TFLOPS**.

Use this during long runs: fans ramp when **util > 5%** or **temp ≥ 70°C** (row highlighted).

### LLM line (dynamic)

`/validate/ping` returns `roles`:

- `coder` — resolved `coder_url` (e.g. `gemma T440:5001`)
- `reviewer` — **mode_state `reviewer_endpoint` first** (e.g. `QwenBig T440:5002`), not draft
- `thinker` — `thinker_url` (ping only)
- `draft` — `draft_endpoint` (e.g. `picasso cesarops2:5571`, ping only)

Example HUD text:

`gemma T440:5001 42.1 tok/s · QwenBig T440:5002 38.2 tok/s · thinker cesarops2:5200 ping · picasso cesarops2:5571 ping`

Tok/s bench runs at most every **45s** so polling does not keep GPUs loaded between pipeline steps.

## Plan cross-reference

- Phase 4d hub SSE — `/hub/stream` includes full `monitor` payload
- Phase 5 golden tasks — use HUD temps + named tok/s during A/B
- Long-run health: watch **cesarops2** rows for 1070/2060 temps, not only T440 P100s

## Requirements

- `cesarops-node` on cesarops2 heartbeating to Forge for remote GPU temps
- Restart Forge after deploy to pick up `/validate/ping` and `/monitor` changes
