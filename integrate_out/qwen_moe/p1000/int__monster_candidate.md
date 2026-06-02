# integrate/unmapped/laptopdump_wreckhunter_build/monster_candidate.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/analysis/monster_candidate.py

## Steps
1. Move `monster_candidate.py` to `/codebase/projects/pipelines/wreckhunter/analysis/`.
2. Refactor script to define `analyze_monster_candidate(target: Target, anomalies: List[Anomaly]) -> CandidateReport`.
3. Replace hardcoded `candidates` list with input parameter or database query interface.
4. Extract `pixels_per_ft` and `monster_mass_tons` to `wreckhunter/constants.py`.
5. Write unit tests for `calculate_target_pixels` and candidate filtering logic in `tests/wreckhunter/test_monster_candidate.py`.
6. Wire `analyze_monster_candidate` into the `wreckhunter` pipeline's candidate evaluation stage via Forge tool configuration.

## Risks
- Hardcoded coordinates and dimensions may conflict with dynamic target registry.
- "Zion Constant" requires definition in domain constants.
- Comparison logic to "Andaste" may be better served by a general comparison module.
