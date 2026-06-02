# CESAROPS Forge — Deployment Runbook

Status as of 2026-05-22. Single-page reference for bringing the forge up on a fresh box.

## What you get
- **Forge web UI** on port `9100` — chat, cluster nodes, mode toggle, ADHD/dyslexia-friendly layout.
- **Tool-calling agent loop** at `POST /cluster/agent/run` — `think_harder`, `wso_search`, `read_file`, `write_file`, `cargo_check`, `run_command`, `remember`.
- **n8n webhook router** on port `5678` — alternative entry point that accepts a tool call and routes to the right backend.

## Architecture

```
┌──────────────────────────────────────────────────────────────────────────┐
│                              FORGE (axum)  :9100                          │
│                                                                            │
│  /             web UI (Lexend, paired-icon labels, semantic colors)        │
│  /cluster      cluster panel                                               │
│  /ide          embedded IDE                                                │
│  /send         conversational send                                         │
│  /code         code-task pipeline (coder → reviewer → integrator)          │
│  /cluster/agent/run   { endpoint, message } → full agent loop with tools   │
│  /cluster/nodes       registered DII nodes                                 │
│  /cluster/discover    legacy probe + tailscale probe                       │
│  /mode (GET) /mode/coding /mode/cesarops    mode toggle                    │
└────────────────┬─────────────────────────────────────────────────────────┘
                 │
        ┌────────┴────────┐
        ▼                 ▼
┌───────────────┐  ┌────────────────────────────────────────────────┐
│ KOBOLDCPP     │  │  TOOLS BACKEND                                   │
│ workers       │  │                                                  │
│  :5001 coder  │  │  nautivecs-cli      :5003   POST /query  (JSON) │
│  :5002 review │  │  cesarops-wso       :5010   POST /search (JSON) │
│  :5570 ui     │  │  nauti-inferer      :8099   GET  /health        │
└───────────────┘  └────────────────────────────────────────────────┘
                                  ▲
                                  │
                          ┌───────┴────────┐
                          │  n8n  :5678    │
                          │  workflow id=1 │
                          │  POST /webhook │
                          │     /tool-route│
                          │                │
                          │  Switch by     │
                          │  tool_name →   │
                          │  Nautivecs |   │
                          │  WSO       |   │
                          │  read_file |   │
                          │  run_cmd       │
                          └────────────────┘
```

## Tool-call payload shape (n8n `/webhook/tool-route`)

```json
{ "tool_call": { "name": "think_harder", "arguments": { "query": "...", "top_k": 5 } } }
{ "tool_call": { "name": "wso_search",   "arguments": { "query": "...", "max_results": 5 } } }
{ "tool_call": { "name": "read_file",    "arguments": { "path": "cesarops-forge-v2/Cargo.toml" } } }
{ "tool_call": { "name": "run_command",  "arguments": { "cmd": "ls /codebase/repos" } } }
```
All return `{ "tool_name": "...", "result": "...", "status": "success" }`.

## Boot order (cold start)

```bash
# 1. Verify Node 20 is on PATH (n8n needs it; system Node 18 breaks oclif)
node --version  # should be v20.x. If not:
export PATH=/home/cesarops/node-v20.18.0-linux-x64/bin:$PATH

# 2. Start tool backends (idempotent, started by their own units)
sudo systemctl start cesarops-wso-server.service       # wso :5010
# nautivecs-cli launches via separate script — check it's running
ss -tlnp | grep 5003 || /home/cesarops/wreckhunter2000-1/target/release/nautivecs-cli \
    --endpoint http://localhost:5001/v1 \
    --db-path /mnt/data-external/cesarops/nautivecs/store.json \
    serve --port 5003 &

# 3. Start LLM workers (whatever fits the box; example for 2x P100)
/home/cesarops/koboldcpp \
    --model /codebase/models/Gemma-4-26B-MoE-IQ4_XS.gguf \
    --usevulkan 0 --gpulayers 999 --tensor_split 1 0 \
    --port 5001 --host 0.0.0.0 --skiplauncher --multiuser 1 --quiet &
/home/cesarops/koboldcpp \
    --model /codebase/models/Qwen2.5-Coder-7B-Instruct-abliterated-Q8_0.gguf \
    --usevulkan 1 --gpulayers 999 \
    --port 5002 --host 0.0.0.0 --skiplauncher --multiuser 1 --quiet &

# 4. Start n8n
nohup /home/cesarops/node-v20.18.0-linux-x64/bin/node \
    /data/n8n/node_modules/n8n/bin/n8n start \
    > /home/cesarops/n8n.log 2>&1 &

# 5. Start forge (systemd-managed today)
sudo systemctl start cesarops-forge-v2.service
```

## Updating the forge UI / binary

The `cesarops-forge-v2.service` runs the binary at:
```
/codebase/repos/wreckhunter2000-1/cesarops-forge-v2/target/release/cesarops-forge-v2
```
But edits live in `/home/cesarops/wreckhunter2000-1/...` (same source tree via shared inode for `src/`, but separate `target/` dirs). Rebuild + push pattern:
```bash
# Rebuild from workspace
cargo build --release -p cesarops-forge-v2 \
    --manifest-path /home/cesarops/wreckhunter2000-1/Cargo.toml

# Push to service path and restart
sudo systemctl stop cesarops-forge-v2
cp /home/cesarops/wreckhunter2000-1/target/release/cesarops-forge-v2 \
   /codebase/repos/wreckhunter2000-1/cesarops-forge-v2/target/release/cesarops-forge-v2
sudo systemctl start cesarops-forge-v2
```

**Better long-term:** point the systemd unit's `ExecStart` at the workspace target dir to drop the copy step.

## Health check

```bash
/home/cesarops/forge_audit.sh    # 18 endpoint probes; expect 16+/18 pass
```

Two endpoints (`/cluster/discover`, `/orchestrator/probe`) take ~25s because they probe network nodes; they're not failures, just slow.

## n8n workflow management

```bash
N=/home/cesarops/node-v20.18.0-linux-x64/bin/node
N8N=/data/n8n/node_modules/n8n/bin/n8n

$N $N8N list:workflow                     # list
$N $N8N export:workflow --id=1 --output=. # export
$N $N8N import:workflow --input=file.json # import (won't update existing — see below)
$N $N8N update:workflow --id=1 --active=true  # activate
```

**Gotcha:** `import:workflow` will not overwrite an existing workflow with the same id. To update an existing workflow, patch sqlite directly:
```python
import sqlite3, json
c = sqlite3.connect('/home/cesarops/.n8n/database.sqlite')
new = json.load(open('n8n_moe_tool_router.json'))
c.execute("UPDATE workflow_entity SET nodes=?, connections=? WHERE id='1'",
          (json.dumps(new['nodes']), json.dumps(new['connections'])))
c.commit()
# then restart n8n
```

## ADHD/dyslexia UI design tokens (already applied to `src/index.html`)

| Token | Value | Why |
|---|---|---|
| Font | Lexend (body) + Atkinson Hyperlegible fallback | Designed for letter recognition |
| Min size | 16px | Below this, dyslexic readers fatigue |
| Letter-spacing | 0.03em | Prevents character "blurring" |
| Line-height | 1.6 | Tight leading is the enemy |
| Max line | 75ch in chat bubbles | Prevents eye-tracking fatigue |
| BG | `#1A1C1E` charcoal (not `#000`) | Avoids halo effect |
| Semantic | green `#2D8A4E` / amber `#D97706` / red `#B91C1C` / blue `#3B82F6` | Distinct, ADA-contrast |
| Destructive separation | CLEAR/STOP/STEER live in `<details>` drawer | Prevents mis-clicks |
| Async ops | Pulsing pending bar + paired icon | Maintains orientation |
| Icons | Always paired with text label | Icons-alone are ambiguous |
| Reduced motion | `@media (prefers-reduced-motion)` honored | Respects OS setting |

## Known issues / wishlist

1. `cluster_config.toml` describes cesarops3 as "P106-100 6GB" but actual hardware is **RTX 2060 SUPER 8GB**. Live `/cluster/nodes` reports correctly; static config is just stale comments.
2. `/orchestrator/probe` and `/cluster/discover` are slow (~25s) due to per-port serial probes. Could be parallelized.
3. Service path duplication (`/codebase/repos/...` vs `/home/cesarops/...`) — pick one as canonical for systemd.
4. PCIe link state on Pascal/Turing cards drops to Gen 1 when idle. Fix with `sudo nvidia-smi -pm 1` (persistence mode) on each box at boot. Add to `/etc/rc.local` or a oneshot service.

## Quick smoke test (one-liner you can run after deploy)

```bash
echo 'health'  ; curl -s http://localhost:9100/health
echo 'mode'    ; curl -s http://localhost:9100/mode | head -c 200
echo 'th'      ; curl -s -X POST http://localhost:5678/webhook/tool-route \
                   -H "Content-Type: application/json" \
                   -d '{"tool_call":{"name":"think_harder","arguments":{"query":"forge","top_k":1}}}' \
                   | head -c 200
```
Expect three OK responses in a few seconds.
