# Predictive Async MoE Pipeline (PAMP)

Fleet-level mixture-of-experts implemented in **n8n** (`:5678`), not in `cesarops-inference`.

## Experts (HTTP endpoints)

| id | URL | Role |
|----|-----|------|
| coder_gemma | `http://127.0.0.1:5001` | Codegen |
| reviewer_mtp | `http://127.0.0.1:5002` | Review |
| thinker_r1 | `http://10.0.0.201:5200` | Plan / intent |
| draft_phi | `http://10.0.0.201:5571` | Speculative draft |
| nautivecs | `http://127.0.0.1:5003/query` | Vectors |
| wso | `http://127.0.0.1:5010/search` | Web |

## Webhooks

- `POST /webhook/tool-route` — single-tool router (existing)
- `POST /webhook/pamp-route` — predict + parallel experts (shadow or live)

## Forge integration

```toml
[orchestration]
tools_backend = "inline"   # or "n8n"
n8n_tool_url = "http://127.0.0.1:5678/webhook/tool-route"
n8n_pamp_url = "http://127.0.0.1:5678/webhook/pamp-route"
pamp_shadow = true         # log plan, keep inline execution
```

## Phase 7d — streaming partial review

When codegen streams, Forge can call n8n with `partial_review: true` after each closed ` ``` ` fence (`orchestration::should_trigger_partial_review`). The Reviewer expert runs on the partial block only; merge if confidence is high.

## Migration

1. Import `missions/n8n_predictive_async_moe.json`
2. Run with `pamp_shadow = true` — scorecard logs `expert_plan`
3. Set `tools_backend = "n8n"` after wall-time win on B5/B3

PAMP does **not** run during `cake_fleet` or `stack=pipeline` missions.
