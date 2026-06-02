# Coral model migration — sigmoid vs logits

## Recommendation (do this)

| Phase | What | Why |
|-------|------|-----|
| **Now** | Run `coral_jitter_worker.py` with **stub_rule** | Laptopdump/repo migration can continue; jitter-rs remote path is live. |
| **Ship v1** | Train **one** int8 model: input `[1,8]`, output **`[1,1]` sigmoid** | Matches stub semantics, simplest Edge TPU compile, one threshold (0.7). |
| **Later (optional)** | Offline A/B: same weights as **2-logit** head | Only if v1 disagrees too often / too rarely on a held-out tile set. |

**Do not run two production models on the ML350e.** Train both formats only for an **offline compare** (see below), pick one, deploy one path under `CORAL_MODEL`.

## Why sigmoid first

- Same “structure probability” story as the §8 stub and `edgetpu.rs` heuristic.
- Smaller graph → faster on Coral, easier int8 quantize.
- `CORAL_OUTPUT=auto` detects `[1,1]` vs `[1,2]` from the tflite — no manual switch in prod.

## When logits are worth it

- Class imbalance (mostly `natural` tiles) and you want a dedicated “structure” logit.
- You need finer disagreement calibration (validator confidence on negatives).

If you train logits, export `[1,2]` with class order **`[natural, structure]`** so softmax index 1 = structure probability (matches sigmoid).

## Offline compare (both exports, one training run)

From the same trained checkpoint:

1. Export **sigmoid** `.tflite` → `jitter_edgetpu_sigmoid.tflite`
2. Export **2-logit** `.tflite` → `jitter_edgetpu_logits.tflite`
3. `edgetpu_compiler` both → deploy only the winner.

```bash
cd cesarops-detection/jitter-rs
python3 tools/compare_coral_outputs.py \
  --sigmoid /path/jitter_edgetpu_sigmoid_edgetpu.tflite \
  --logits  /path/jitter_edgetpu_logits_edgetpu.tflite
```

Metrics printed: agreement rate vs synthetic primary, mean |Δcertainty|, p99 latency on CPU tflite.

**Pick winner by:**

1. Higher agreement with held-out labels (if you have them), and  
2. Stable disagreement cases (not always 0.05 or always 0.9), and  
3. p99 infer &lt; 50 ms on device after compile.

## Production env (ML350e)

```bash
export CORAL_OUTPUT=auto          # default — detects from model
export CORAL_MODEL=/opt/cesarops/models/jitter_edgetpu_edgetpu.tflite
bash scripts/start_coral_jitter_worker.sh
```

## T440 (during migration)

Keep remote validator pointed at ML350e; stub backend is fine until v1 model lands:

```bash
export JITTER_REMOTE_VALIDATORS="coral_edgetpu=http://<ml350e-ip>:8190"
```

Consensus already degrades cleanly if the worker is slow or stub-only.
