# integrate/unmapped/laptopdump_wreckhunter_build/config_agent.py

## Verdict
PORT_TO_PIPELINES

## Target path
tools/config_wizard.py

## Steps
1. Extract target profiles (weights, thresholds, and sensor configurations) into a new `pipelines/core/config_defaults.py` module.
2. Refactor the `ConfigAgent` class to import these constants instead of hardcoding them in methods.
3. Replace the hardcoded Windows path (`C:\Users\thomf\...`) with a configurable environment variable or a relative path from the project root.
4. Implement `tools/config_wizard.py` as the entry point, utilizing the refactored `ConfigAgent`.
5. Add a test suite in `tests/test_config_logic.py` to validate that the weight/threshold mappings are mathematically correct.

## Risks
* **Hardcoded Paths:** The source contains absolute Windows paths that will break on Linux/Docker environments.
* **Blocking I/O:** The use of `input()` makes this script incompatible with automated/headless CI/CD pipelines.
* **Logic Drift:** If the weights in the tool are updated without updating the core pipeline logic, detection results will be inconsistent.
