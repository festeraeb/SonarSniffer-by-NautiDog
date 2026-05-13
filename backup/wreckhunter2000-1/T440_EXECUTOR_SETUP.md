# T440 ExecutionWorker Setup & Split-Agent Workflow

## Overview

The **T440 at 10.0.0.61** is a 6GB-class node dedicated to **code generation** in the CESARops distributed system. It runs `split-agent` in **daemon mode**, receiving architectural briefs from the ReasoningLead (i7) and returning generated Rust/WGSL code.

```
i7 (8GB+)              T440 (6GB)
ReasoningLead          ExecutionWorker
┌──────────────┐       ┌──────────────┐
│ --mode       │       │ --mode       │
│  reason      ├──────>│  daemon      │
│              │ JSON  │              │
│ (plans)      │ brief │ (generates)  │
└──────────────┘       └──────────────┘
                    POST /brief
                    ← code response
```

## Quick Start

### 1. Update .env with T440 configuration

```bash
# In .env (if not already present)
T440_HOST=10.0.0.61
T440_USER=executor
T440_WORK=/home/executor/wreckhunter2000-1
CODING_BASE_URL=http://localhost:5001/v1  # T440's local LLM endpoint
```

### 2. Deploy to T440

```bash
# Full deployment: sync + build + install + systemd
bash scripts/deploy_t440_execution_worker.sh

# Or step-by-step:
bash scripts/deploy_t440_execution_worker.sh --sync-only
bash scripts/deploy_t440_execution_worker.sh --build-only
bash scripts/deploy_t440_execution_worker.sh --restart
```

### 3. Verify T440 is running

```bash
# From your machine:
curl http://10.0.0.61:8766/health
curl http://10.0.0.61:8766/ready

# Or via SSH:
ssh executor@10.0.0.61 'systemctl status split-agent-daemon'
```

## Workflow Modes

### Single-Node Mode (SuperAgent 24GB+)

```bash
./target/release/model-team-tool \
  --mode auto \
  --task "Refactor nauticuvs for synthetic grids"
```
Detects hardware → runs reasoning + coding + review locally.

### Distributed Mode

#### ReasoningLead (i7, 8GB+)

```bash
# Step 1: Generate a TechnicalBrief
./target/release/model-team-tool \
  --mode reason \
  --task "Refactor nauticuvs for synthetic grids" \
  --reasoning-url http://localhost:5001/v1 \
  --reasoning-temperature 0.2 \
  --output brief.json
```

#### ExecutionWorker (T440, 6GB) — Daemon

```bash
# Runs automatically via systemd, but can also start manually:
./target/release/model-team-tool \
  --mode daemon \
  --port 8766 \
  --coding-url http://localhost:5001/v1
```

#### Dispatch from ReasoningLead

```bash
# Send the brief to the T440 daemon
curl -X POST \
  -H "Content-Type: application/json" \
  -d @brief.json \
  http://10.0.0.61:8766/brief
```

### Using the Orchestration Script

```bash
# From your machine (or i7):
bash scripts/orchestrate_codegen.sh \
  --reasoning http://10.0.0.56:5001/v1 \
  --execution http://10.0.0.61:8766 \
  --task "Optimize tile slicer for synthetic grids" \
  --output results.json
```

This script:
1. Runs reasoning phase locally (generates TechnicalBrief)
2. Dispatches brief to T440 ExecutionWorker
3. Extracts generated code and saves to `results.rs`

## Environment Variables

### On T440 (in systemd service):

- `CODING_BASE_URL` — Local LLM endpoint (default: `http://localhost:5001/v1`)
  - Must point to a running inference server (KoboldCpp, vLLM, Ollama, etc.)
- `RUST_LOG` — Logging level (default: `info`)
  - Set to `debug` for verbose output

### On i7 (ReasoningLead):

- `REASONING_BASE_URL` — Local reasoning model endpoint
  - If not set, defaults to `http://localhost:5001/v1`
- When dispatching, override with `--reasoning-url` flag

## Hardware Detection

The split-agent tool auto-detects VRAM and selects the role:

| VRAM | Role | Mode | Use Case |
|------|------|------|----------|
| 24GB+ | SuperAgent | `--mode auto` | Full pipeline locally |
| 8-23GB | ReasoningLead | `--mode reason` | Planning only |
| 6-7GB | ExecutionWorker | `--mode daemon` | Code generation only |
| <6GB | FallbackCPU | `--mode full` (API-only) | CPU-only fallback |

T440 has ~6GB, so it will be detected as `ExecutionWorker`.

## API Endpoints (T440 Daemon)

```
GET /health              Check daemon is alive
GET /ready               Check if ready to accept briefs
POST /brief              Accept a TechnicalBrief, return code response
```

### POST /brief Request

```json
{
  "task": "...",
  "architecture": "...",
  "approach": "...",
  "key_decisions": ["...", "..."],
  "constraints": ["...", "..."],
  "file_targets": ["src/foo.rs", "src/bar.rs"],
  "context": "..."
}
```

### POST /brief Response

```json
{
  "status": "success",
  "code": "// Generated Rust code here",
  "file_path": "src/foo.rs",
  "reasoning": "Why this approach...",
  "integration_notes": "..."
}
```

## Troubleshooting

### T440 daemon won't start

```bash
# SSH to T440 and check logs:
ssh executor@10.0.0.61
journalctl -u split-agent-daemon -n 50

# Common issues:
# - Port 8766 already in use: lsof -i :8766
# - LLM endpoint not reachable: curl http://localhost:5001/v1/health
# - VRAM detection failed: split-agent --mode auto (test manually)
```

### Curl to /brief fails

```bash
# Check T440 is listening:
ssh executor@10.0.0.61 'netstat -tlnp | grep 8766'

# Check brief.json is valid JSON:
jq . brief.json

# Test with a minimal brief:
echo '{"task":"test"}' | curl -X POST -H "Content-Type: application/json" -d @- http://10.0.0.61:8766/brief
```

### Code generation is slow

- Check `CODING_BASE_URL` is pointing to a fast endpoint (KoboldCpp with GPU acceleration is ~60 tokens/sec)
- Monitor VRAM on T440: `ssh executor@10.0.0.61 'nvidia-smi'`
- Adjust `coding_temperature` (lower = more deterministic, faster)

## Integration with CESARops Pipeline

The T440 ExecutionWorker integrates into the larger CESARops flow:

1. **Discovery Phase**: T440 announces itself via mDNS on startup
   - i7 and other nodes can discover T440's capabilities
2. **Task Dispatch**: When i7 needs code generation:
   - i7 runs `--mode reason` or calls orchestration script
   - Brief is sent to T440 daemon
   - Generated code is returned and integrated into the pipeline
3. **Idle Scout**: T440 can also run its own idle scanner (if desired)
   - See `sovereign-cloud` for multi-pass pipeline examples

## Performance Targets

| Operation | Time | Notes |
|-----------|------|-------|
| Hardware detect | <100ms | NVML or wgpu probe |
| Planning phase (reason) | 10-30s | Depends on task complexity |
| Code generation (code) | 15-60s | Depends on code size (~600 tokens) |
| End-to-end (single-node) | 30-90s | SuperAgent with full pipeline |

## Next Steps

1. Deploy T440 and verify it's running
2. Test planning phase on i7: `--mode reason --task "..."`
3. Send a test brief to T440: `orchestrate_codegen.sh "test task"`
4. Integrate with n8n orchestration on the Pi dispatcher

---

**Node Configuration Summary:**
- **T440 (10.0.0.61)**: ExecutionWorker daemon (port 8766)
- **i7 (10.0.0.56)**: Sovereign-Cloud API (port 8765) + optional reasoning phase
- **Pi (10.0.0.226)**: n8n dispatcher (orchestrates workflows)
- **XENON (10.0.0.129)**: KoboldCpp LLM server (port 5001)
