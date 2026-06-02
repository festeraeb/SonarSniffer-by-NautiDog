# integrate/unmapped/laptopdump_wreckhunter_build/pull_altimetry_anonymous.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/altimetry_ingestion/

## Steps
1. Create directory structure: `pipelines/altimetry_ingestion/{src,tests,config}`.
2. Refactor `pull_altimetry_anonymous.py` into `src/puller.py`, replacing all `print` statements with standard `logging`.
3. Extract hardcoded constants (`GREAT_LAKES_BBOXES`, `SATELLITE_FTP_PATHS`, `AVISO_FTP_HOST`) into `config/config.yaml`.
4. Replace `argparse` logic with a `PipelineTask` entry point that reads from the pipeline runner's context.
5. Replace the local `OUTPUT_BASE` logic with a configurable `data_dir` parameter from the pipeline environment.
6. Implement unit tests in `tests/test_puller.py` using `unittest.mock` to simulate `ftplib.FTP` and `requests.get` responses.
7. Wire the task into the main pipeline orchestrator.

## Risks
* **Network/Auth:** AVISO/NASA/PO.DAAC may change FTP/HTTPS credentials or access protocols (e.g., requiring OAuth2 instead of anonymous).
* **Storage:** NetCDF (`.nc`) files are large; unmanaged downloads could exhaust pipeline disk space.
* **Data Integrity:** The script relies on regex for date extraction from filenames; changes in satellite naming conventions will break filtering.
* **Bounding Box Accuracy:** The hardcoded Great Lakes coordinates may require precision updates for specific satellite footprints.
