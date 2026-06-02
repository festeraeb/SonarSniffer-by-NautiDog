# integrate/unmapped/laptopdump_wreckhunter_build/tpu_server.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/wreckhunter/services/tpu_server.py

## Steps
1. Create directory structure: `/codebase/projects/pipelines/wreckhunter/services/` and `/codebase/projects/pipelines/wreckhunter/models/`.
2. Move `tpu_server.py` to the new service path.
3. Create `requirements.txt` containing: `flask`, `numpy`, `Pillow`, `tflite-runtime`.
4. Create a `Dockerfile` using a Debian-based image that installs `libedgetpu1-std` to support the Coral hardware.
5. Update `MODEL_PATH` in `tpu_server.py` to ensure it points correctly to the relative `../models/` directory.
6. Replace `app.run()` in the `if __name__ == '__main__':` block with a production-ready entrypoint (e.g., `gunicorn --bind 0.0.0.0:5001 tpu_server:app`).
7. Add a test suite that validates `stub_inference` works in environments without TPU hardware (CI/CD compatibility).

## Risks
* **Hardware Dependency**: The service requires physical Coral TPU access; deployment to standard cloud runners will trigger the `stub_inference` fallback.
* **Library Conflicts**: `tflite-runtime` can conflict with full `tensorflow` installations in the same environment.
* **Driver Requirements**: Requires `libedgetpu.so.1` to be present in the runtime environment/container.
