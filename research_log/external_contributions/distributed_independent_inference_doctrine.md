# Distributed Independent Inference (DII) Doctrine

**Status:** Accepted — this is the architectural direction for CESAROPS multi-node inference.  
**Date:** May 17, 2026  
**Source:** Operator architectural analysis (verbatim reasoning preserved below)  
**Cross-references:**
- `web_compute_layer_v1_design.md` — earlier "Compute Exchange" concept (DII subsumes this)
- `multi_model_v1_registry_scheduler.md` — registry/scheduler design (DII extends this)
- `homelab_strategy_v1_doctrine.md` — hardware strategy (DII is the software counterpart)

---

## Core Distinction

**What we are NOT building:**
- Distributed tensor parallel (NCCL-style cross-device activation transfer)
- Synchronized attention across nodes
- Pipeline parallelism with bubble overhead
- KV sync / tensor shards / all-reduce

**What we ARE building:**
- Distributed independent inference orchestration
- Each node owns complete model execution
- Orchestration happens at request granularity
- Network transfers are ONLY: prompts, logits, completions, health/state metadata

This distinction matters enormously on:
- PCIe-only systems (no NVLink)
- Mixed VRAM fleets (4 GB to 16 GB)
- Pascal cards (decent fp16, awful modern interconnect)
- Homelab WAN/LAN nodes
- Heterogeneous accelerators (GPU + TPU + CPU)

---

## Why This Is Correct for Pascal

Pascal has:
- Decent fp16 throughput
- Good memory bandwidth
- Awful modern interconnect story
- No tensor cores
- Weak synchronization economics

Therefore:
- Local inference = efficient
- Distributed tensor compute = terrible

We optimize the right layer.

---

## Architecture

```
web frontend / cesarops.com
        ↓
   orchestrator (forge :9100)
        ↓
   node registry (cesarops-node daemons)
        ↓
   workers (independent full-model runtimes)
```

NOT:
```
frontend → split tensor execution across machines
```

---

## Scheduling Strategy

**Queue-per-model, NOT queue-per-node.**

Because:
- Multiple nodes may host the same model
- Nodes may host multiple models (different ports)

```
model: qwen-coder
    ↓
available workers: [P100#0, cesarops3]
```

### Request routing table:

| Request Type | Best Node |
|:---|:---|
| Coding | Qwen coder node (P100) |
| Long context | Gemma node (P100) |
| Fast chat | Tiny Mistral / TinyLlama (P1000) |
| Reranker | TPU node (Coral) |
| Embeddings | CPU AVX node |

Without ever moving weights. That's the power.

---

## KV-Prefix-Aware Routing

If a node already has:
- System prompt + lorebook + session cache

Route continuation requests there. Minimizes prefill cost across the fleet.
That's where orchestration starts outperforming naive load balancing.

---

## Cross-Node Speculative Decoding

```
small fast node proposes → big slow node verifies
```

Exactly like local speculative decoding — except network-distributed.
Strong fit for:
- P100 + 1070 mixed clusters
- TPU sidecars
- CPU-only fallback nodes

---

## Fault Isolation

If one node crashes/OOMs/wedges Vulkan/loses network:
- You lose ONE request
- NOT the entire tensor graph

Massive operational win vs. tensor-parallel.

---

## Node Daemon Architecture

Each machine runs `cesarops-node`:

Responsibilities:
- Register capabilities
- Advertise models
- Heartbeat
- Expose inference endpoint
- Local scheduler
- VRAM accounting

Example registration payload:
```json
{
  "node_id": "p100-west-01",
  "models": [
    {
      "name": "qwen2.5-coder-1.5b-q6k",
      "ctx": 8192,
      "quant": "Q6_K",
      "tok_s": 18.2
    }
  ],
  "hardware": {
    "gpu": "Tesla P100",
    "vram_gb": 16
  }
}
```

---

## Central Orchestrator Responsibilities

1. **Registry** — tracks alive nodes, models, throughput, queue depth, latency, trust score
2. **Scheduler** — decides where requests go, batching opportunities, speculative pairings
3. **Aggregation** — can merge outputs, voting, reranking, speculative verification

---

## Recommended Stack

| Layer | Technology |
|:---|:---|
| Control plane | Rust + Axum + Postgres |
| Worker transport | gRPC or QUIC |
| Metrics | Prometheus |
| Queue | NATS or Redis streams |
| Auth | Ed25519 node identity |

---

## What the Website Should Be

Not "a chatbot website."

It should be: **"Compute Exchange"**

Users contribute GPU/TPU/CPU/RAM/storage cache in exchange for priority, credits, tokens, inference quota.

That's the economically sustainable version.

---

## Security Model

- Sandboxed workers — never trust arbitrary nodes
- Workers should never receive model weights unless authorized
- Receive signed jobs, return signed outputs
- Ed25519 node identity for all registration/heartbeat

---

## What to Avoid

- Tensor parallel over Ethernet
- NCCL-style distributed inference
- Synchronized attention across nodes

That only works economically with NVLink, InfiniBand, datacenter GPUs. Not our target fleet.

---

## What to Pursue

Our strongest niche: **"Distributed independent inference for abandoned datacenter GPUs"**

Especially if:
- Vulkan/wgpu native
- Pascal optimized
- Kobold-compatible
- Single binary
- Volunteer node federation

That combination is genuinely differentiated.

---

## Implementation Priority

1. Node capability registry (`cesarops-node` daemon) ← IN PROGRESS
2. Scheduler (queue-per-model routing in orchestrator)
3. Remote inference protocol (standardized /spawn /stop /status)
4. Credit/quota system (for Compute Exchange)
5. KV-prefix-aware request routing (minimize prefill across fleet)

---

## Lessons for `research_log/lessons_learned.md`

- DII > tensor-parallel for Pascal/PCIe/homelab — never attempt NCCL-style sync
- Queue-per-model routing outperforms queue-per-node for heterogeneous fleets
- Cross-node speculative decoding is viable when latency < 50ms between nodes
- Fault isolation is the #1 operational advantage of DII over tensor-parallel
- KV-prefix routing is the key to beating naive load balancing at scale
