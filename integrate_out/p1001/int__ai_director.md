# integrate/unmapped/laptopdump_programming_root/ai_director.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/agents/ai_director.py

## Steps
1. Create `/codebase/projects/pipelines/agents/` directory.
2. Refactor `subprocess` calls to use relative paths from the pipeline root rather than `__file__` to ensure stability in containerized environments.
3. Replace all `input()` calls with mandatory CLI arguments or environment variables to prevent headless execution hangs.
4. Map the `AVAILABLE_TOOLS` dictionary to the pipeline's central tool registry/manifest.
5. Replace the custom `.env` loading logic with the fleet's standard `SecretManager` or `ConfigProvider` interface.
6. Implement a `dry_run` mode to validate LLM-generated plans without executing `subprocess` commands.

## Risks
* **Non-determinism:** LLM-based tool selection and parameter tuning can lead to unpredictable pipeline behavior.
* **Execution Fragility:** Heavy reliance on `subprocess` makes the agent sensitive to changes in the file system or Python environment.
* **Blocking I/O:** The current use of `input()` will cause automated/CI/CD pipelines to hang indefinitely.
* **Latency/Cost:** Integration of Qwen introduces external API latency and variable operational costs.
