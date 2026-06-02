# integrate/unmapped/laptopdump_wreckhunter_build/tpu_client.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/utils/tpu_client.py

## Steps
1. Create `utils` directory within the pipelines project if not present.
2. Port the `TPUClient` class to the new location.
3. Refactor `_image_to_bytes` to ensure strict type handling for `numpy` arrays (common in T440 pipelines).
4. Remove the `if __name__ == "__main__":` block and the `test_tpu_server` function; move these to a dedicated test file in `/tests/`.
5. Add `requests` and `Pillow` to the pipeline's `requirements.txt`.
6. Wire the client into the main image processing loop to allow remote glint/jitter validation via the Coral TPU server.

## Risks
* **Network Dependency:** The client fails gracefully but requires a stable connection to the TPU server to provide actual inference results.
* **Latency:** Base64 encoding and HTTP overhead may introduce bottlenecks in high-throughput image processing streams.
* **Server State:** Client assumes the server is running; if the server crashes, the pipeline defaults to `pass=True`, which could allow bad data through.
