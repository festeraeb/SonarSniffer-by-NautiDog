# Model Team Tool (Standalone)

Rust standalone tool to run:
- reasoning model (planner + reviewer)
- coding model (implementation generator)

Both endpoints are OpenAI-compatible (KoboldCPP, vLLM, LM Studio, etc.).

## Build

```powershell
cargo build --release
```

## Run

```powershell
.\target\release\model-team-tool.exe --task "Refactor module X"
```

## Environment variables

- `REASONING_BASE_URL` default `http://127.0.0.1:5001/v1`
- `REASONING_MODEL` default `deepseek-r1-distill-qwen-7b`
- `REASONING_API_KEY` default `not-needed`
- `CODING_BASE_URL` default `http://127.0.0.1:5002/v1`
- `CODING_MODEL` default `qwen2.5-coder-7b-instruct`
- `CODING_API_KEY` default `not-needed`

## Installer project

- Inno Setup script: `packaging/model-team-tool.iss`
- Build helper: `packaging/build-installer.ps1`
