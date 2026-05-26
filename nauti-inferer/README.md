# NautiInferer v4

Distributed inference API. Routes jobs to **RTX 2060 / GTX 1070 / P100** via Forge fleet discovery.  
Default: **avoid P100** when `avoid_p100: true` (thinker workloads go to cesarops2).

## Quick start

### Coordinator (T440)

```bash
cd /codebase/repos/wreckhunter2000-1
cargo build --release -p nauti_inferer

export FORGE_URL=http://127.0.0.1:9100
export NAUTI_LISTEN_PORT=8099
./target/release/nauti-inferer
```

### Worker heartbeat (cesarops2 — optional, improves registry)

```bash
export NAUTI_MODE=worker
export NAUTI_COORDINATOR_URL=http://10.0.0.61:8099
export LOCAL_INFERENCE_URL=http://127.0.0.1:5200
export NAUTI_WORKER_ID=RTX2060
export NAUTI_WORKER_ROLE=thinker
./target/release/nauti-inferer
```

Ensure llama-server is running on cesarops2 `:5200` first.

## API

| Route | Description |
|-------|-------------|
| `GET /health` | Service health |
| `GET /v1/nodes` | Fleet nodes (Forge sync + workers) |
| `GET /v1/models` | Model labels |
| `POST /v1/inference` | Chat inference (SSE or JSON) |
| `POST /v1/inference/cancel/:id` | Cancel job |
| `POST /internal/fleet/sync` | Force Forge resync |

### Example (thinker on RTX 2060, skip P100)

```bash
curl -N -X POST http://127.0.0.1:8099/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{
    "prefer_role": "thinker",
    "avoid_p100": true,
    "stream": true,
    "messages": [{"role":"user","content":"Summarize NautiInferer Phase 3 in one paragraph."}]
  }'
```

Non-stream:

```bash
curl -s -X POST http://127.0.0.1:8099/v1/inference \
  -H 'Content-Type: application/json' \
  -d '{
    "prefer_role": "thinker",
    "avoid_p100": true,
    "stream": false,
    "messages": [{"role":"user","content":"Say OK"}]
  }' | jq .
```

## Phases implemented

- **Phase 1:** scheduler, node registry, heartbeat, quotas (SQLite)
- **Phase 2:** HTTP API, SSE streaming, cancel, worker register/heartbeat
- **Phase 3:** Forge `/cluster/gpus` sync, llama `/v1/chat/completions` adapter, role-based routing (2060 thinker)

Phase 4 (API keys + credits) not yet implemented.
