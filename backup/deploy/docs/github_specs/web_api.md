# SonarSniffer Web API (Prototype)

This document describes the minimal FastAPI-based web backend scaffold that serves outputs and provides admin endpoints for regenerating the index and running the Holloway pipeline.

Environment variables

- SONARS_OUTPUT_DIR: root outputs directory (default: repository `outputs/`)
- SONARS_API_TOKEN: if set, admin endpoints (`POST /api/*`) require this value in header `X-Token`
- SONARS_HOST: host to bind (default 127.0.0.1)
- SONARS_PORT: port to run the server (default 8081)

Endpoints

- GET / -> serves the built-in UI prototype (`/ui/index.html`) when present, otherwise serves the generated `outputs/holloway_smoke/index.html` if available, else a tiny landing page.
- GET /api/outputs -> returns JSON list of files under `SONARS_OUTPUT_DIR` with basic metadata.
- POST /api/outputs/regenerate (admin) -> runs `scripts/generate_outputs_index.py` in the background.
- POST /api/run/holloway (admin) -> start a background Holloway pipeline run (returns `job_id`).
- GET /api/run/{job_id} -> job status and recent logs.
- GET /api/logs/{name} -> fetch a log file by name from outputs dir.

UI

- Static prototype UI is mounted under `/ui/`. The root `/` redirects to `/ui/index.html` when the prototype exists.
Running locally

1. Install deps: `pip install -r requirements.txt`
2. Start server: `python scripts/serve_web.py`
3. Open `http://127.0.0.1:8081/` in your browser to view the minimal UI (it will fetch `/api/outputs`).

Notes

- This is a prototype scaffold intended to be extended: add WebSocket endpoints for live logs, auth improvements, job persistence, and a richer SPA/UI.
- For production deployment, serve via a reverse proxy (nginx) and consider using a Redis-backed job queue for long-running processes.
