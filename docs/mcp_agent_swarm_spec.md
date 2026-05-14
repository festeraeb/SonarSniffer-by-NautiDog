# MCP Agent Swarm: Self-Organizing GPU Workers

## Overview

Each GPU worker becomes an MCP server that advertises its specialty. A coordinator agent (running on the bootstrap brain or forge) can:
1. See what workers are online and what they specialize in
2. See what GGUF models are available on disk
3. See what GPU VRAM is free
4. Decide which model to load on which GPU based on the task
5. Route sub-tasks to the right specialist

## Architecture

```
┌─────────────────────────────────────────────────────────┐
│              COORDINATOR (forge-v2 / bootstrap)          │
│  - Sees all workers via MCP discovery                    │
│  - Knows what models are on disk (/codebase/models/)     │
│  - Knows GPU VRAM status (nvidia-smi)                    │
│  - Decides: "I need a code reviewer → load Phi-3 on     │
│    the 1060, it fits in 6GB"                             │
│  - Routes tasks to specialists via MCP tool calls        │
└────────────┬──────────────┬──────────────┬──────────────┘
             │              │              │
    ┌────────▼────┐  ┌─────▼──────┐  ┌───▼────────┐
    │  P100 #0    │  │  1070      │  │  1060      │
    │  MCP Server │  │  MCP Server│  │  MCP Server│
    │             │  │            │  │            │
    │ Specialty:  │  │ Specialty: │  │ Specialty: │
    │ "coder"     │  │ "reviewer" │  │ "thinker"  │
    │             │  │            │  │            │
    │ Model:      │  │ Model:     │  │ Model:     │
    │ Qwen3.6-35B │  │ Phi-3-14B  │  │ R1-7B     │
    │             │  │            │  │            │
    │ Tools:      │  │ Tools:     │  │ Tools:     │
    │ write_file  │  │ read_file  │  │ think      │
    │ read_file   │  │ review     │  │ reason     │
    │ cargo_check │  │ suggest    │  │ plan       │
    │ run_command │  │            │  │            │
    └─────────────┘  └────────────┘  └────────────┘
```

## Worker Self-Description

Each MCP worker advertises a capability manifest:

```json
{
  "worker_id": "p100-0",
  "hostname": "t440cesarops",
  "gpu": "Tesla P100-PCIE-16GB",
  "vram_total_mb": 16384,
  "vram_free_mb": 14200,
  "specialty": "coder",
  "model_loaded": "Qwen3.6-35B-A3B-MXFP4_MOE.gguf",
  "tools": ["write_file", "read_file", "cargo_check", "run_command", "think_harder", "remember"],
  "safe_mode": false,
  "status": "idle",
  "capabilities": [
    "rust_code_generation",
    "tool_calling",
    "multi_file_editing",
    "cargo_build_verification"
  ]
}
```

## Coordinator Decision Logic

The coordinator picks models based on:

```rust
struct ModelFit {
    model_path: String,
    size_mb: u64,
    specialty: String,       // what this model is good at
    min_vram_mb: u64,        // minimum VRAM needed
    template: String,        // chat template (qwen, phi, deepseek-r1, etc.)
    quantization: String,    // Q4_K_M, Q6_K, Q8_0, F16, etc.
}

// Model registry (scanned from /codebase/models/ at startup)
// Each model has metadata about what it's good at:
// - Qwen3.6-35B → coder, tool_calling
// - DeepSeek-R1-7B → reasoning, planning
// - Phi-3-14B → review, polish, instruction_following
// - Qwen2.5-Coder-7B → code_generation, fast
// - Fortytwo_Strand-14B → rust_specialist

// Decision: given a task + available GPUs, pick the best model
fn select_model_for_task(
    task: &Task,
    available_gpus: &[GpuStatus],
    model_registry: &[ModelFit],
) -> Option<(ModelFit, GpuId)> {
    // 1. Filter models that fit in available VRAM
    // 2. Score by specialty match to task type
    // 3. Prefer models already loaded (no swap cost)
    // 4. Return best (model, gpu) pair
}
```

## MCP Server Implementation (per worker)

Each worker runs an MCP server using `rmcp`:

```rust
use rmcp::{Server, Tool, tool};

#[derive(Clone)]
struct CesaropsWorker {
    specialty: String,
    model_name: String,
    gpu_id: usize,
    safe_mode: bool,
    project_root: String,
}

#[tool(name = "write_file", description = "Write content to a file")]
async fn write_file(path: String, content: String) -> String { ... }

#[tool(name = "read_file", description = "Read a file's content")]
async fn read_file(path: String) -> String { ... }

#[tool(name = "run_command", description = "Execute a shell command")]
async fn run_command(cmd: String) -> String { ... }

#[tool(name = "cargo_check", description = "Run cargo check on a directory")]
async fn cargo_check(dir: String) -> String { ... }

#[tool(name = "generate", description = "Generate text using the loaded LLM")]
async fn generate(prompt: String, max_tokens: u32) -> String { ... }

#[tool(name = "get_capabilities", description = "Report this worker's specialty and status")]
async fn get_capabilities() -> String { ... }
```

## Coordinator MCP Client

The coordinator connects to all workers and orchestrates:

```rust
// Coordinator discovers workers via Tailscale + port scanning
// or via a registry (cluster_config.toml known_nodes)

async fn dispatch_task(task: &Task, workers: &[McpClient]) -> TaskResult {
    // 1. Ask each worker for capabilities
    // 2. Find best match for task type
    // 3. If no specialist loaded, decide to swap:
    //    - Check what models fit on free GPUs
    //    - Load the best model for this task
    //    - Wait for model to be ready
    // 4. Send task to chosen worker via MCP tool calls
    // 5. Collect result
}
```

## Model Swap Protocol

When the coordinator decides a different model is needed:

1. Check if target GPU is idle (no active generation)
2. Kill current koboldcpp/inference process on that GPU
3. Start new process with the chosen model
4. Wait for /health endpoint to respond
5. Update worker registry with new specialty
6. Route task to newly loaded worker

## Integration with Forge v2

The forge cluster panel already has:
- GPU status monitoring (`/monitor`)
- Worker start/stop (`/cluster/worker/{idx}/start|stop`)
- Model listing (`/cluster/models`)
- Freeform role assignment

Add:
- `POST /cluster/auto-assign` — let coordinator pick models automatically
- `GET /cluster/workers/capabilities` — get all worker manifests
- `POST /cluster/swap` — swap a model on a specific GPU

## Crate Dependencies

```toml
[dependencies]
rmcp = { version = "0.1", features = ["server", "transport-sse", "transport-stdio"] }
```

## File Structure

```
cesarops-mcp-worker/
├── Cargo.toml
├── src/
│   ├── main.rs              — CLI: --port, --gpu, --specialty
│   ├── server.rs            — MCP server setup + tool registration
│   ├── tools.rs             — Tool implementations (reuse from forge)
│   ├── capabilities.rs      — Self-description manifest
│   ├── model_registry.rs    — Scan /codebase/models/, parse metadata
│   └── coordinator.rs       — Decision logic for model selection
```
