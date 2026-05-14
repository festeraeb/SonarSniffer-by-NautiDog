# Files to Delete

## Assessment Criteria
Files marked for deletion are either:
- Empty stubs with no implementation
- Exact duplicates of files already in the deploy structure
- Old versions superseded by newer working copies
- Files that serve no purpose in the current architecture

| File | Reason |
|------|--------|
| backup/src/cesarops-forge/ | Superseded by cesarops-forge-v2; old forge version with no active development |
| backup/src/thought-engine/ | Superseded by cesarops-thought-engine/; standalone copy is redundant |
| backup/src/tauri/ | Likely outdated frontend framework; cesarops-forge-web provides modern web UI |
| backup/src/tauri-mission-control/ | Superseded by cesarops-mission-control/; tauri wrapper around same functionality |
| backup/src/src/ | Duplicate workspace source directory; contents overlap with individual crate src/ dirs |
| backup/src/research_log/ | Log directory, not executable code; logs should be generated at runtime |
| backup/src/nauticuvs/ | Unclear purpose; if it's a duplicate of nautivecs or unused module, delete after verification |
| backup/src/searxng/ | External search engine instance config; may be needed but likely managed externally |
| backup/src/wrecks_api/ | If API endpoints are now served through cesarops-forge-v2, this is deprecated |
| backup/src/model-team-tool/ | Single-purpose tool; check if functionality merged into agent or tools modules |
| backup/src/GEMINI_WGPU_SPEC.md | Spec document for Gemini integration; reference only, move to docs/ then delete from root |
| backup/src/SCM_RUN7_ANALYSIS.txt | Analysis artifact; move to docs/, original can be deleted |
| backup/src/SONAR_SNIFFER_ANALYSIS.md | Analysis artifact; move to docs/, original can be deleted |
| backup/src/IMPLEMENTATION_ROADMAP.md | Move to deploy/docs/; keep as roadmap reference |
| backup/src/INVENTORY.md | Move to deploy/docs/; keep as inventory reference |
