# integrate/unmapped/laptopdump_wreckhunter_build/drive_identity.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/utils/provisioning/drive_identity.py

## Steps
1. Create directory `/codebase/projects/pipelines/utils/provisioning/`.
2. Move `drive_identity.py` to the new path.
3. Refactor `WEBPAGE_API` constant to pull from an environment variable `CESAROPS_API_URL` or a central `config/settings.json` to avoid hardcoding.
4. Extract the hardcoded SQL schema in `verify_database` into a standalone `schema.sql` file in the same directory.
5. Update `verify_database` to read from the external `.sql` file.
6. Add `requests` to the project `requirements.txt`.
7. Refactor `get_or_create_drive_id` to accept an optional `owner` argument to allow for headless/automated provisioning (removing the `input()` call).
8. Run integration test: `python3 drive_identity.py ./test_drive_mount` and verify `drive_id.json` and folder structure creation.

## Risks
* **Hardcoded Endpoint:** The `WEBPAGE_API` URL is a placeholder and will fail if not updated.
* **Interactive Blocking:** The `input()` call for "owner name" will hang in automated CI/CD or headless environments.
* **Network Dependency:** The registration step requires an active internet connection to the Cloudflare Worker.
* **Dependency Management:** Requires `requests` which may not be in the base T440 environment.
