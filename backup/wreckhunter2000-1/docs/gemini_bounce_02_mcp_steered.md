# Bounce 02: cesarops-mcp-steered — For Gemini

## What Kiro Built (current state in repo)

### Files created:
- `cesarops-mcp-steered/Cargo.toml` — dependencies (nautivecs as path dep, reqwest, clap, tokio, serde)
- `cesarops-mcp-steered/src/main.rs` — CLI config + entry point (clap with env var fallbacks)
- `cesarops-mcp-steered/src/llm_client.rs` — OpenAI-compatible client (chat completions, health check)
- `cesarops-mcp-steered/src/steering.rs` — nautivecs context injection engine (multi-query fusion, role-based search, anti-drift grounding rules)
- `cesarops-mcp-steered/src/tools.rs` — MCP JSON-RPC over stdio with 6 tools

### Tools exposed:
1. `steered_query` — general grounded questions (nautivecs injects relevant code)
2. `tune_parameters` — threshold/config decisions with sensor-focused context
3. `analyze_code` — code analysis with full project context injection
4. `reindex` — refresh the nautivecs store with latest code changes
5. `health` — check LLM endpoint + nautivecs status
6. `execute_shader` — dispatch wgpu/WGSL compute to GPU cluster (dipole_scan, curvelet_filter, thermal_submersion, optical_structural, nauticus_scan, spectral_analysis)

### Key design decisions made:
- **No external MCP SDK** — we roll our own JSON-RPC over stdio (~100 lines). The Rust MCP ecosystem is too young to depend on.
- **Configurable endpoint** — env vars or CLI args: `LLM_URL`, `LLM_MODEL`, `LLM_API_KEY`, `NAUTIVECS_DB`, `EMBEDDING_URL`, `CONTEXT_BUDGET`
- **Injection format** — markdown code blocks with file paths (what `InjectedContextBuilder` already produces). LLMs parse this natively.
- **Anti-drift enforcement** — HYBRID approach: hard rules for parameter values/function names (must exist in context), soft rules for architecture/approach (can reason beyond context)
- **Multi-query fusion** — don't just search the raw question. Decompose into technical terms, add role-specific queries, merge results. This is in `steering.rs::derive_search_queries()`

---

## What We Need From Gemini (Next Iteration)

### 1. Human Feedback Loop (NEW — critical feature)

We want an n8n worker that:
- Presents LLM decisions to the human in a simple web form
- Collects feedback: "correct / wrong / threshold too aggressive / function doesn't exist / drifted off topic"
- Stores corrections as HIGH-PRIORITY fragments in nautivecs
- On future queries, corrections are injected FIRST (before regular code context)

**The flow:**
```
MCP tool call → LLM decision → stored in feedback_queue
                                        ↓
n8n workflow polls feedback_queue → presents to human via web form
                                        ↓
Human responds: "wrong — glint threshold 0.9 is too high for calm water"
                                        ↓
n8n webhook → POST /feedback to cesarops-mcp-steered
                                        ↓
Correction stored in nautivecs with score=2.0 (higher than normal fragments at 0.3-0.9)
                                        ↓
Next time LLM sees similar query, it gets:
  "HUMAN CORRECTION (2026-05-07): glint threshold 0.9 was reported as too aggressive
   for calm water conditions. User suggested 0.4-0.6 range. Adjust accordingly."
```

**This is RLHF without retraining** — corrections live in the vector store as high-priority context.

**Questions for Gemini:**
- Should corrections expire? (e.g., after 30 days, reduce priority from 2.0 to 1.0?)
- Should corrections be scoped? (e.g., "this correction only applies to calm water tiles" vs global)
- What's the n8n webhook payload format? I'm thinking:

```json
{
  "decision_id": "uuid",
  "tool_name": "tune_parameters",
  "original_query": "...",
  "llm_response": "...",
  "feedback": "wrong",
  "correction": "glint threshold should be 0.4-0.6 for calm water",
  "scope": "weather_window=calm",
  "severity": "high"
}
```

### 2. The `execute_shader` Tool — How Should It Dispatch?

The MCP server needs to trigger wgpu compute on remote GPU nodes. Options:

**Option A: HTTP to sovereign-cloud API (port 8765)**
- Already exists, already has dispatch endpoints
- MCP server → HTTP POST → sovereign-cloud → wgpu dispatch
- Pro: no new infrastructure. Con: extra hop, sovereign-cloud must be running

**Option B: Direct wgpu in the MCP server process**
- MCP server opens its own wgpu device and runs shaders locally
- Pro: zero network latency. Con: MCP server must run on a GPU machine

**Option C: Hybrid — try local wgpu first, fall back to remote dispatch**
- If MCP server has a GPU → run locally
- If not (e.g., running on laptop without GPU) → dispatch to cluster via HTTP
- Pro: works everywhere. Con: more code paths

**My vote: Option C** — matches the "any node can do any role" philosophy. What does Gemini think?

### 3. MCP Protocol Implementation

I rolled my own JSON-RPC stdio handler. It works but it's basic. The protocol needs:
- `initialize` / `initialized` handshake
- `tools/list` — return tool schemas
- `tools/call` — execute a tool and return result
- Streaming support (for long LLM responses) — `tools/call` with progressive content

**Question for Gemini:** Should we add streaming support now, or is request/response sufficient for v0.1? The LLM responses can take 5-30 seconds depending on model size. Without streaming, the MCP client (Kiro) just waits.

### 4. Nautivecs Store Bootstrapping

Before the MCP server is useful, the nautivecs store needs to be indexed with the codebase. Currently this is manual (`nautivecs-cli index ./src`).

**Proposal:** On first startup, if the store is empty, auto-index the workspace:
```rust
if steering.chunk_count() == 0 {
    tracing::warn!("nautivecs store is empty — auto-indexing workspace...");
    steering.reindex(Path::new(".")).await?;
}
```

Should we also add a file watcher that re-indexes on code changes? Or is manual `reindex` via the tool sufficient?

### 5. Correction Store Schema

For the human feedback loop, we need a separate collection in the nautivecs JSON store (or a sibling file):

```json
{
  "corrections": [
    {
      "id": "uuid",
      "created_at": "2026-05-07T15:30:00Z",
      "tool_name": "tune_parameters",
      "original_query": "tune glint threshold for calm Erie tiles",
      "llm_response_summary": "suggested 0.9",
      "feedback": "wrong",
      "correction_text": "HUMAN CORRECTION: glint threshold 0.9 is too aggressive for calm water. Use 0.4-0.6.",
      "scope": {"weather_window": "calm", "sensor": "sentinel2"},
      "priority": 2.0,
      "expires_at": null,
      "applied_count": 0
    }
  ]
}
```

When building context, corrections matching the current query scope are injected FIRST with highest priority.

---

## Architecture Diagram (Updated)

```
┌─────────────────────────────────────────────────────────────────┐
│                        Kiro IDE                                   │
│  (or any MCP client: Claude Desktop, custom frontend)            │
└──────────────────────────────┬──────────────────────────────────┘
                               │ MCP stdio (JSON-RPC)
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│              cesarops-mcp-steered (Rust binary)                   │
│                                                                   │
│  ┌─────────────┐  ┌──────────────┐  ┌────────────────────────┐ │
│  │ Tool Router │  │ Steering     │  │ LLM Client             │ │
│  │ (JSON-RPC)  │→ │ Engine       │→ │ (OpenAI-compat)        │ │
│  │             │  │ (nautivecs)  │  │ KoboldCPP/Cake/mistral │ │
│  └─────────────┘  └──────┬───────┘  └────────────────────────┘ │
│                           │                                      │
│  ┌────────────────────────┴──────────────────────────────────┐  │
│  │              Correction Store                              │  │
│  │  (high-priority fragments from human feedback)             │  │
│  └────────────────────────┬──────────────────────────────────┘  │
│                           │                                      │
│  ┌────────────────────────┴──────────────────────────────────┐  │
│  │              Shader Dispatcher                             │  │
│  │  Local wgpu (if GPU) → OR → HTTP to sovereign-cloud       │  │
│  └───────────────────────────────────────────────────────────┘  │
└──────────────────────────────┬──────────────────────────────────┘
                               │ HTTP webhook
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                    n8n (Human Feedback)                           │
│                                                                   │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────┐  │
│  │ Decision     │  │ Web Form     │  │ Correction            │  │
│  │ Queue        │→ │ (approve/    │→ │ Webhook               │  │
│  │ (poll MCP)   │  │  reject/fix) │  │ (POST /feedback)      │  │
│  └──────────────┘  └──────────────┘  └──────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## What's Already Working (can test now)

1. **nautivecs** — the vector store crate is built and has passing tests (`nautivecs/tests/pipeline_integration.rs`)
2. **n8n** — running on cesarops2 (configured in `.roo/mcp.json`)
3. **LLM endpoint** — KoboldCPP on cesarops2 at 94.7 tok/s (TinyLlama), Cake/mistral.rs building
4. **Cloudflare tunnel** — `llm.cesarops.org` routes to T440 (when online), `app.cesarops.org` serves frontend from cesarops3

## What Needs Building (priority order)

1. **Get the MCP server compiling** — resolve the `mcp-server` crate dependency (may need to vendor or roll our own)
2. **Wire nautivecs into steering.rs** — the integration code is written but untested
3. **Add `submit_feedback` tool + correction store** — the human loop
4. **n8n workflow** — poll decisions, present form, POST corrections back
5. **Test anti-drift** — index the codebase, ask questions, verify grounding works
6. **Shader dispatch** — wire `execute_shader` to sovereign-cloud HTTP API

---

## Questions Summary (for Gemini to address)

1. Should corrections expire or stay forever?
2. Should corrections be scoped (per weather/sensor/region) or global?
3. Shader dispatch: Option A (HTTP), B (local wgpu), or C (hybrid)?
4. Streaming MCP responses: now or later?
5. Auto-index on first startup: yes or no?
6. n8n webhook format: is the proposed JSON good or does it need more fields?
7. Should the n8n form show the injected context too (so the human can see WHAT the LLM was grounded on)?

---

## Kiro MCP Config (ready to use once binary is built)

```json
{
  "mcpServers": {
    "cesarops-steered": {
      "command": "./target/release/cesarops-mcp",
      "args": [],
      "env": {
        "LLM_URL": "https://llm.cesarops.org/v1",
        "LLM_MODEL": "qwen3-8b",
        "NAUTIVECS_DB": "./data/nautivecs_store.json",
        "CONTEXT_BUDGET": "4096"
      }
    }
  }
}
```
