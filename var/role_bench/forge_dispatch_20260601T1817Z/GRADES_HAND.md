# Hand grades — Forge routing + thinker dispatch (three coders)

**Run dir:** `var/role_bench/forge_dispatch_20260601T1817Z`  
**Polisher:** skipped (per request)

## Forge routing (applied)

```json
{
  "thinker_endpoint": "http://10.0.0.201:5200",
  "coder_endpoint": "http://10.0.0.61:5001",
  "draft_endpoint": "http://10.0.0.61:5002",
  "corrector_endpoint": "http://10.0.0.201:5202",
  "reviewer_endpoint": "http://10.0.0.61:5002"
}
```

Saved via `POST /cluster/routing` — see `forge_routing.json`.

**Models at run** (`endpoints_at_run.json`):

| Role | URL | Loaded model |
|------|-----|----------------|
| Thinker (RTX) | `:5200` | Qwen3.6-35B-A3B-MXFP4_MOE |
| Coder A (P100) | `:5001` | Gemma-4-26B-MoE-IQ4_XS |
| Coder B (P100) | `:5002` | Qwen3.6-35B-A3B-Q4_K_M |
| Coder C (1070) | `:5202` | gemma-4-E4B-it-Q4_K_M *(expected Qwen2.5-Coder-7B)* |

Re-apply anytime:

```bash
bash scripts/forge_apply_thinker_dispatch_routing.sh
```

---

## Dynamic watchdog request (for Forge — from thinker attempt)

**Status:** Thinker on RTX MoE did **not** emit clean `## Dynamic watchdog request for Forge` in `content` (hit `max_tokens` in `reasoning_content` only).  

**Intent captured from thinker reasoning** (v1 run, `thinker_dispatch.md`):

- Replace `mission_service_watchdog.sh` with a **dynamic** restorer.
- Input: heartbeat snapshot keyed by **`gpu_uuid` + `port` + `model_path`** (+ timestamp).
- Per GPU: pick **latest** snapshot → **stop/unload port** → relaunch **same port** with last `model_path`.
- No fixed triple-stack preset; idempotent; log transitions.

**Your hand grade (watchdog spec):** _____ /10  

**Notes:**

---

## Thinker as dispatcher (RTX `:5200`)

| Metric | Value |
|--------|--------|
| Words | 1039 (mostly reasoning meta; **0** usable `##` sections in content) |
| Time | ~573s |
| Handoffs parseable? | **No** |

**Suggested grade: 18 / 100** — did not produce dispatchable worker sections; wrong role fit for MoE+reasoning on this prompt.

**Your grade:** _____ /100

**Fix for next run:** load **Gemma-4-E4B** on `:5200` with `--reasoning off`, or raise tokens and forbid reasoning channel.

---

## Coder A — P100 Gemma MoE (`:5001`)

**File:** `coder_P100-Gemma-MoE.md` (271 words)

**Expected slice:** UX/script inventory + CLI spec for dynamic watchdog (`inventory/UX_SPEC.md`).

**Got:** Generic `wreckhunter2000-1` Python adapter (`engine.py`, `wreckhunter_adapter.py`) — **not** script inventory / watchdog UX.

**Suggested grade: 35 / 100** — competent code sketch, **ignored thinker handoff** (empty).

**Your grade:** _____ /100

---

## Coder B — P100 Qwen3.6 (`:5002`)

**File:** `coder_P100-Qwen36.md` (229 words after direct schema prompt; first dispatch call **HTTP 500**)

**Expected slice:** Heartbeat JSON schema + bash reviewer for watchdog snapshots.

**Got:** Valid **JSON schema draft** for `gpu_uuid`, `port`, `model_path`, `timestamp` + start of bash reviewer outline — but only after **fallback prompt** (not thinker handoff). Content still in `reasoning_content` style preamble.

**Suggested grade: 62 / 100** — best **schema** alignment; dispatch path failed; needs `reasoning off` + shorter prompt.

**Your grade:** _____ /100

---

## Coder C — 1070 (`:5202`)

**File:** `coder_1070-Qwen25-Coder7B.md` (278 words)

**Expected slice:** `cesarops-detection` / `process_tile` sketch.

**Got:** Generic `WreckHunter` salvage entity — **no** `cesarops-detection`, no `process_tile`, no repo paths.

**Note:** Port had **Gemma E4B**, not Qwen2.5-Coder-7B — routing vs loaded model mismatch.

**Suggested grade: 30 / 100**

**Your grade:** _____ /100

---

## Overall pipeline verdict

| Step | Pass? |
|------|-------|
| Forge routing | ✅ |
| Thinker → structured handoffs | ❌ (RTX MoE) |
| Three coders on-mission | ❌ (1/3 partial via fallback) |
| Polisher | — skipped |

**Next test:** RTX **Gemma E4B** thinker + verify `:5202` has **Qwen2.5-Coder-7B** before dispatch.

---

## Artifacts

| File | Description |
|------|-------------|
| `thinker_dispatch.md` | v1 thinker (reasoning-heavy) |
| `watchdog_request_from_thinker.md` | empty — extract from reasoning manually |
| `coder_*.md` | Coder outputs |
| `forge_routing.json` | Routing POST result |

**Re-run:**

```bash
bash scripts/forge_apply_thinker_dispatch_routing.sh
THINKER_URL=http://127.0.0.1:5200 bash scripts/role_bench/run_thinker_dispatch_three_coders.sh
```
