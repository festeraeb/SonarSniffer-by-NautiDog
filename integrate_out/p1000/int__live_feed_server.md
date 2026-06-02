# integrate/unmapped/laptopdump_wreckhunter_build/live_feed_server.py

## Verdict
PORT_TO_PIPELINES

## Target path
/codebase/projects/pipelines/live_feed_server/live_feed_server.py

## Steps
1. Create directory `/codebase/projects/pipelines/live_feed_server/`.
2. Port `detection_sorter.py` and all associated logic/modules into the new directory.
3. Refactor `DB_PATH` in `live_feed_server.py` to use `os.getenv('CESAROPS_DB_PATH')`.
4. Refactor `API_KEYS` in `detection_sorter.py` to use environment variables or a secure secret store.
5. Create `requirements.txt` containing `flask`, `simplekml`, and `gunicorn`.
6. Update `app.secret_key` to strictly use `os.environ`.
7. Validate KMZ generation and API response integrity via `pytest`.

## Risks
* **Security**: `API_KEYS` are currently hardcoded in the source; must be moved to environment variables.
* **Path Fragility**: The current `DB_PATH` uses relative `__file__` logic which will break in containerized/orchestrated environments.
* **Dependency Management**: Requires `simplekml` which is not part of the standard library.
