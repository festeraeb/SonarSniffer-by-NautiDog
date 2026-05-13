# Model Team Tool (Rust)

This repo now includes a standalone Rust tool that runs two LLM roles as a team:

- Reasoning model: plans and reviews
- Coding model: executes implementation output

It does not replace existing embedded flows; it is additive.

## Binary

- Cargo bin: `model_team_tool`
- File: `cesarops-slicer/src/model_team_tool.rs`

## Quick run

```bash
cargo run -p cesarops-slicer --bin model_team_tool -- --task "Add a new SAR post-processing stage"
```

## Environment variables

- `REASONING_BASE_URL` (default `http://127.0.0.1:5001/v1`)
- `REASONING_MODEL` (default `deepseek-r1-distill-qwen-7b`)
- `REASONING_API_KEY` (default `not-needed`)
- `CODING_BASE_URL` (default `http://127.0.0.1:5002/v1`)
- `CODING_MODEL` (default `qwen2.5-coder-7b-instruct`)
- `CODING_API_KEY` (default `not-needed`)

## n8n workflow

- Import file: `model_team_n8n_workflow.json`
- Webhook path: `POST /webhook/model-team-run`
- Body:

```json
{
  "task": "Refactor llm_worker to separate prompt templates."
}
```

## IDE tooling (JetBrains + VS Code)

Use this stack for practical coding-agent workflows:

- `Continue` extension/plugin (both IDE families) for multi-model chat and edit loops
- `CodeGPT` or `Cline` (VS Code) for model routing and task execution
- `JetBrains AI Assistant` for in-editor explain/refactor/test generation
- `n8n` + webhook for reproducible external orchestration
- `Taskfile` or `justfile` to standardize model-team commands
- `pre-commit` + `ruff` + `cargo clippy` to gate agent output quality
