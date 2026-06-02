# integrate/unmapped/laptopdump_programming_root/ai_director.py

## Verdict
MERGE_INTO_LIVE

## Target path
/codebase/projects/pipelines/ai_director/ai_director.py

## Steps
1. **Create Directory**: `mkdir -p /codebase/projects/pipelines/ai_director/`
2. **Move File**: `mv integrate/unmapped/laptopdump_programming_root/ai_director.py /codebase/projects/pipelines/ai_director/ai_director.py`
3. **Refactor Imports**: Replace hardcoded sibling script paths (`lake_michigan_scan.py`, `triple_lock_fusion.py`) with absolute imports (e.g., `from pipelines.lake_michigan_scan import ...`) or ensure they are co-located in the same package.
4. **Remove Interactive I/O**: Replace `input()` calls in `check_and_prompt_api_keys()` with `argparse` flags or raise `RuntimeError` if keys are missing. Automated pipelines cannot handle interactive prompts.
5. **Strip Platform-Specific Code**: Remove the `sys.platform == 'win32'` encoding block; the fleet runs on Linux.
6. **Configure Secrets**: Update `.env` loading to use fleet secret management (e.g., `os.environ` or `secrets_manager`) instead of local file parsing.
7. **Add Tests**: Create `test_ai_director.py` to verify `parse_request` logic and `set_bounding_box` validation without invoking external LLMs.

## Risks
- **External Dependency**: Relies on Qwen API; requires API key provisioning and rate-limit handling.
- **Hardcoded Sibling Scripts**: Assumes `lake_michigan_scan.py` and `triple_lock_fusion.py` exist; verify their canonical paths.
- **Interactive Blocking**: Original code blocks on `input()`; must be fully non-interactive for pipeline use.
- **State Management**: Writes `.env` and `outputs/` to script directory; must be redirected to `/tmp` or fleet scratch space.
