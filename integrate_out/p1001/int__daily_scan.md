# integrate/unmapped/laptopdump_wreckhunter_build/daily_scan.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/daily_scan.py

## Steps
1. **Relocate**: Move script to the wreckhunter pipeline directory.
2. **Refactor Paths**: Replace hardcoded `Path(__file__).parent` logic with a centralized `config.py` or environment variables to support T440 P100 filesystem standards.
3. **Update Imports**: Adjust `from detection_sorter import ...` and `from cesarops_engine import ...` to use absolute package imports (e.g., `from cesarops.engine import ...`) based on the actual codebase structure.
4. **Parameterize**: Convert the `tiff_files[:10]` testing limit into a configurable parameter (e.g., `MAX_TILES_PER_RUN`).
5. **Forge Integration**: Replace the `cron` comment with a Forge task definition for automated daily execution.
6. **Validation**: Run `pytest` on the `process_tile` integration to ensure the engine handles the loop correctly.

## Risks
* **Path Fragility**: The current script relies heavily on relative directory structures which will fail in the production environment without refactoring.
* **Dependency Mismatch**: The `cesarops_engine` and `detection_sorter` modules may have different signatures in the live codebase compared to the laptop dump.
* **Resource Exhaustion**: If the 10-tile limit is removed without monitoring, CUDA/TPU memory usage during the `process_tile` loop could crash the node.
