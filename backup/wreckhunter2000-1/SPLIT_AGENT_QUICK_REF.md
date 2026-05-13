# CESARops Split-Agent Distributed Workflow — Quick Reference

## Architecture at a Glance

```
n8n Dispatcher (Pi 10.0.0.226)
    ↓
    Orchestrates via mDNS
    ↓
┌─────────────────────────────────────────────────────────────┐
│  Sovereign Cloud Network (Tailscale overlay)                │
├─────────────────────────────────────────────────────────────┤
│                                                              │
│  i7 (10.0.0.56)                 T440 (10.0.0.61)           │
│  ReasoningLead                  ExecutionWorker             │
│  8GB VRAM, Coral TPU            6GB VRAM, no GPU            │
│  ┌────────────────┐             ┌────────────────┐          │
│  │ sovereign-     │◄────Brief───│ split-agent    │          │
│  │ cloud port     │  (TechnicalBrief JSON)│ daemon           │
│  │ 8765           │─────Code───>│ port 8766      │          │
│  │                │             │                │          │
│  │ --mode reason  │             │ --mode daemon  │          │
│  │ (planning)     │             │ (execution)    │          │
│  └────────────────┘             └────────────────┘          │
│                                                              │
│  XENON (10.0.0.129)                                         │
│  KoboldCpp LLM Server (port 5001, 5002)                    │
│  ↑ Used by both ReasoningLead and ExecutionWorker          │
│                                                              │
└─────────────────────────────────────────────────────────────┘
```

## Deployment Status

| Node | Role | Component | Port | Status | Deployment |
|------|------|-----------|------|--------|------------|
| i7 | ReasoningLead | sovereign-cloud | 8765 | ✓ Active | `deploy_sovereign.sh` |
| T440 | ExecutionWorker | split-agent daemon | 8766 | 🔧 Setup | `deploy_t440_execution_worker.sh` |
| XENON | LLM Server | KoboldCpp | 5001, 5002 | ✓ Active | Manual |
| Pi | Dispatcher | n8n | 80/443 | ✓ Active | n8n setup |

## Quick Start: T440 Deployment

```bash
# 1. Update .env with T440 details
nano .env
# Fill in:
#   T440_HOST=10.0.0.61
#   T440_USER=executor
#   CODING_BASE_URL=http://localhost:5001/v1

# 2. Deploy to T440
bash scripts/deploy_t440_execution_worker.sh

# 3. Verify
curl http://10.0.0.61:8766/health
```

## Workflow: Single Request to Generate Code

### Option A: Auto-Detect (Single Node)

```bash
./target/release/model-team-tool \
  --mode auto \
  --task "Add LanceDB support to tile_store"
```

**Best for**: Development machines with 24GB+ VRAM.

### Option B: Distributed (i7 → T440)

```bash
# Step 1: ReasoningLead plans the work
./target/release/model-team-tool \
  --mode reason \
  --task "Add LanceDB support to tile_store" \
  --reasoning-url http://10.0.0.129:5001/v1 \
  --output brief.json

# Step 2: ExecutionWorker generates code
curl -X POST \
  -H "Content-Type: application/json" \
  -d @brief.json \
  http://10.0.0.61:8766/brief > code_response.json

# Step 3: Extract results
jq -r '.code' code_response.json > generated.rs
```

### Option C: Orchestration Script (Automated)

```bash
bash scripts/orchestrate_codegen.sh \
  --reasoning http://10.0.0.129:5001/v1 \
  --execution http://10.0.0.61:8766 \
  --task "Add LanceDB support to tile_store" \
  --output results.json
```

**Best for**: Production pipelines via n8n.

## Environment Variables

### T440 (Systemd Service)

```bash
CODING_BASE_URL=http://localhost:5001/v1
RUST_LOG=info
```

### i7 (ReasoningLead)

```bash
REASONING_BASE_URL=http://10.0.0.129:5001/v1  # or override with --reasoning-url
LLM_BASE_URL=http://10.0.0.129:5001/v1
```

### XENON (KoboldCpp)

```bash
# Terminal 1: Reasoning model (on port 5001)
koboldcpp --port 5001 --model deepseek-r1-7b.gguf --usecublas

# Terminal 2: Coding model (on port 5002)
koboldcpp --port 5002 --model qwen2.5-coder-7b.gguf --usecublas
```

## Hardware Role Detection

```rust
match total_vram_gb {
    24.. => AgentRole::SuperAgent,         // Full pipeline
    8..24 => AgentRole::ReasoningLead,     // Planning only
    6..8 => AgentRole::ExecutionWorker,    // Code generation only
    _ => AgentRole::FallbackCpu,           // API-only
}
```

## API Reference

### T440 Daemon Endpoints

```
GET  /health          → {"status": "ok"}
GET  /ready           → {"ready": true}
POST /brief           → Accepts TechnicalBrief, returns CodeGenResponse
```

### TechnicalBrief Structure

```json
{
  "task": "Add LanceDB support to tile_store",
  "architecture": "Trait-based TileStore abstraction with LanceDB backend",
  "approach": "Implement TileStore trait, use lancedb crate for vector ops",
  "key_decisions": [
    "Use lancedb for semantic search",
    "Maintain compatibility with existing SledTileStore",
    "Lazy-load vectors on access"
  ],
  "constraints": [
    "Must compile with wgpu on T440",
    "Cannot introduce external Python dependencies"
  ],
  "file_targets": ["src/tile_store.rs", "nauticuvs/src/vector_db.rs"],
  "context": "Sovereign Cloud multi-pass pipeline needs fast vector search for ROI matching"
}
```

### CodeGenResponse Structure

```json
{
  "status": "success",
  "code": "// Generated Rust code here...",
  "file_path": "src/tile_store.rs",
  "reasoning": "Used trait pattern to allow multiple backends...",
  "integration_notes": "Requires 'lancedb' crate in Cargo.toml",
  "metrics": {
    "tokens_generated": 1200,
    "inference_time_ms": 45000
  }
}
```

## Troubleshooting

### T440 won't start

```bash
ssh executor@10.0.0.61
journalctl -u split-agent-daemon -n 50 -f  # Follow logs

# Check port:
netstat -tlnp | grep 8766

# Check VRAM:
nvidia-smi
```

### Brief dispatch fails

```bash
# Validate JSON:
jq . brief.json

# Test endpoint directly:
curl -v http://10.0.0.61:8766/ready
```

### LLM endpoint unreachable

```bash
# From T440:
curl http://localhost:5001/v1/models

# From i7:
curl http://10.0.0.129:5001/v1/models

# Check XENON:
ssh -t xenon@10.0.0.129 'ps aux | grep kobold'
```

## n8n Integration

### Workflow: Code Generation Task

1. **n8n receives task** (from webhook or schedule)
2. **HTTP Request node** → ReasoningLead:
   - POST `http://10.0.0.56:8765/v1/pipeline/dispatch`
   - Body: `{"task": "...", "mode": "reason"}`
3. **HTTP Request node** → ExecutionWorker:
   - POST `http://10.0.0.61:8766/brief`
   - Body: output from step 2
4. **Extract code** and save to database or file share

See `n8n_codegen_workflow.json` (to be created) for example.

## Performance Baseline

| Operation | Duration | Target Node |
|-----------|----------|------------|
| Hardware detect | <100ms | T440 or i7 |
| Reasoning (planning) | 15-30s | i7 (8GB) |
| Code gen (execution) | 20-45s | T440 (6GB) |
| Full auto (single) | 40-90s | i7+ or laptop (24GB+) |

## Next Steps

- [ ] Deploy T440 and verify `/health`
- [ ] Test planning phase locally on i7
- [ ] Test code dispatch from i7 to T440
- [ ] Integrate into n8n pipeline
- [ ] Monitor VRAM usage on both nodes
- [ ] Set up CI/CD to validate generated code

---

**Questions?** See `T440_EXECUTOR_SETUP.md` for detailed troubleshooting.
