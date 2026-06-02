# integrate/unmapped/laptopdump_wreckhunter_build/tpu_server.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/tpu_server.py

## Steps
1.  Create directory `/codebase/projects/pipelines/wreckhunter/`.
2.  Copy `tpu_server.py` to `/codebase/projects/pipelines/wreckhunter/tpu_server.py`.
3.  Create `/codebase/projects/pipelines/wreckhunter/models/` and add `glint_jitter.tflite` (or symlink to fleet model store).
4.  Create `/codebase/projects/pipelines/wreckhunter/requirements.txt` with `flask`, `pillow`, `numpy`, `tflite-runtime`.
5.  Add tests in `/codebase/projects/pipelines/wreckhunter/tests/test_tpu_server.py` covering `stub_inference` and mocked `run_tpu_inference`.
6.  Refactor `tpu_server.py` to use environment variables for `MODEL_PATH`, `PORT`, and `USE_TPU`.
7.  Add `Dockerfile` for containerized deployment on Xenon servers.

## Risks
- `libedgetpu.so.1` dependency may fail on Xenon servers without Coral TPU drivers.
