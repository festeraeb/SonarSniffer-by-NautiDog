# integrate/unmapped/laptopdump_wreckhunter_build/monster_analysis.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/monster_analysis.py

## Steps
1. Create directory `/codebase/projects/pipelines/wreckhunter/`.
2. Refactor `MONSTER_SITE` and `ANCASTE_SITE` into a configuration YAML or JSON file to decouple data from logic.
3. Extract `haversine_distance`, `calculate_mass`, and `classify_object` into a `utils/geospatial.py` module within the pipeline.
4. Rewrite `analyze_monster_site` to accept input data (JSON/Dict) rather than relying on hardcoded global constants.
5. Replace hardcoded local Windows paths (`C:\Users\thomf\...`) with environment-aware `Path` objects using `os.getenv('OUTPUT_DIR', './outputs')`.
6. Implement a CLI entry point using `argparse` to allow running analysis on different target datasets.
7. Integrate with Forge tool to automate KML generation as a post-processing step in the pipeline.

## Risks
* **Hardcoded Data**: The current script mixes analysis logic with specific site data; failure to decouple will prevent reuse for other wreck sites.
* **Pathing**: The script contains absolute Windows-style paths which will break in the Linux-based T440 environment.
* **Floating Point Precision**: Mass estimation relies on heuristic ratios (L/B, L/D) which are approximations and may require calibration against known wreck datasets.
