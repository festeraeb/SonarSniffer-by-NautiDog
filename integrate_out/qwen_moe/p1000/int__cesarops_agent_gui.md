# integrate/unmapped/laptopdump_wreckhunter_build/cesarops_agent_gui.py

## Verdict
PORT_TO_PIPELINES

## Target path
`tools/cesarops_agent_gui.py`

## Steps
1. Move `integrate/unmapped/laptopdump_wreckhunter_build/cesarops_agent_gui.py` to `tools/cesarops_agent_gui.py`.
2. Replace hardcoded `wreckhunter2000/data/cache` in `load_tiles` with a configurable path (e.g., `config.get("tile_cache_dir")` or `os.environ.get("CESAROPS_TILE_CACHE")`).
3. Implement `run_laptop` to invoke existing processing functions (e.g., `cesarops.processing.run` or `wreckhunter.run`).
4. Implement `push_xenon` to use the existing Xenon client library for configuration upload.
5. Add `tools/` to `sys.path` or ensure package structure allows import if the GUI needs to import internal modules.
6. Add `tools/cesarops_agent_gui.py` to `requirements.txt` if `tkinter` is not guaranteed (though usually standard).

## Risks
- Hardcoded path `wreckhunter2000/data/cache` will break on other machines.
- Core functionality (`run_laptop`, `push_xenon`) is stubbed and requires implementation.
- GUI dependencies (`tkinter`) may not be available in headless or minimal environments.
- No unit tests for the GUI logic or configuration handling.
- Presets are hardcoded in memory; consider persisting them to a file.
