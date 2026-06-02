# CESAROPS n8n Orchestration Layer

Turns the three Rust detection pipelines into LLM-driveable, automatable
specialists. A user describes what they're looking for; an LLM router picks the
right pipeline(s) and knobs from each tool's `--describe` catalog; n8n runs the
pipeline and returns the findings.

```
  user ──▶ /webhook/wreck-search  (orchestrator)
              │  LLM plans: which pipeline(s) + knobs + stages
              ▼
        specialist sub-workflow (satellite | aeromag | bag)
              │  Execute Command → Rust binary --json
              ▼
        MissionReport JSON ──▶ LLM synthesises ──▶ user report
```

## Files

| File | Purpose |
|------|---------|
| `pipeline_runner.sh`        | Bridges n8n → Rust binaries (describe / run / preflight) |
| `workflows/orchestrator.json` | LLM router webhook — plans and dispatches |
| `workflows/specialist_satellite.json` | Satellite specialist sub-workflow |
| `workflows/specialist_aeromag.json`   | Aeromagnetic specialist sub-workflow |
| `workflows/specialist_bag.json`       | BAG specialist sub-workflow |
| `tool_catalog.json`         | Cached `--describe` output of all 3 pipelines (LLM context) |
| `import_workflows.sh`       | Imports the workflow JSONs into a running n8n |

## Quick start

```bash
# 1. Regenerate the tool catalog from the live binaries
bash n8n/pipeline_runner.sh catalog > n8n/tool_catalog.json

# 2. Start n8n (if not already running)
#    n8n listens on :5678; webhooks at /webhook/<path>
N8N_PORT=5678 n8n start    # or your existing n8n service

# 3. Import the workflows
bash n8n/import_workflows.sh

# 4. Drive it
curl -s -X POST http://127.0.0.1:5678/webhook/wreck-search \
  -H 'Content-Type: application/json' \
  -d '{"query":"look for the Colgate whaleback in eastern Lake Erie with magnetics","dry_run":true}'
```

## Design notes
- Each pipeline is **self-describing** (`--describe`/`describe`), so the LLM
  router needs no hardcoded knob knowledge — it reads `tool_catalog.json`.
- `pipeline_runner.sh` is the single trust boundary: it validates the pipeline
  name, builds the binary path, and shells the Rust tool. n8n's Execute Command
  node calls it so the workflows stay declarative.
- Dry-run is plumbed end-to-end so the LLM can plan without spending compute.
