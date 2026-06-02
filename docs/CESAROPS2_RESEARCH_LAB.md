# cesarops2 research lab (CPU detection + dual-GPU LLM)

When cesarops3 scout / real vision workers are busy, run pipeline integration tests on **cesarops2** without tying up GPUs for Florence-2 or Moondream.

## Layout

| Service | Port | Hardware | Role |
|---------|------|----------|------|
| CPU sim Scout | 5570 | CPU | `POST /analyze` — numpy heuristics |
| CPU sim Validator | 5572 | CPU | `POST /validate` |
| CPU sim Jitter | 8080 | CPU | `POST /jitter` |
| cesarops-detection | 5580 | CPU | Triple-lock orchestrator |
| llama-server coder | 5200 | CUDA0 (2060 SUPER) | Qwen2.5-Coder-14B Q4 |
| llama-server draft | 5571 | CUDA1 (1070) | Phi-3-mini Q4 |

Validator sim uses **5572** so **5571** stays available for Phi-3 draft LLM (Forge `draft_endpoint`).

## Commands (on cesarops2)

```bash
export REPO=/mnt/t440/codebase/repos/wreckhunter2000-1
bash "$REPO/scripts/cesarops2_research_lab.sh" start
bash "$REPO/scripts/cesarops2_research_lab.sh" status
bash "$REPO/scripts/cesarops2_research_lab.sh" test-detection
bash "$REPO/scripts/cesarops2_research_lab.sh" test-llm
bash "$REPO/scripts/cesarops2_research_lab.sh" stop
```

## Forge / cluster endpoints (LAN)

From T440 or laptop, hit cesarops2 at **10.0.0.201**:

- Coder: `http://10.0.0.201:5200/v1/chat/completions`
- Draft: `http://10.0.0.201:5571/v1/chat/completions`
- Detection: `http://10.0.0.201:5580/health`

Update `cesarops-forge-v2/cluster_config.toml` `corrector_endpoint` / `draft_endpoint` to `10.0.0.201` when testing.

## CPU sim behavior

Not trained wreck detection — returns plausible JSON so you can test:

- Job submit / poll on `:5580`
- Full **3-lock Confirmed** when tile has contrast + `thermal_timeseries` length ≥ 2
- Worker health in `GET /health`

Implementation: `cesarops-detection/workers/cpu_sim_workers.py`
