# Docker deployment for SonarSniffer

This document describes how to run SonarSniffer in Docker for reproducible deployments.

Build locally

```bash
# Build image
docker build -t sonarsniffer:local .
# Run with outputs mounted
docker run --rm -p 8081:8081 -v $PWD/outputs:/app/outputs sonarsniffer:local
```

Run with docker-compose

```bash
docker-compose up --build
```

CI

- A GitHub Actions workflow `docker-build.yml` is provided to build and push multi-arch images to GHCR.
- Set `GHCR_PAT` in repository secrets with a personal access token that can push to GHCR.

Notes

- Production deployments should add a reverse proxy (nginx) and TLS (Let's Encrypt).
- For GPU-accelerated pipelines, the image must include drivers/plugins and the container should be run with `--gpus` or `--device` flags. We include a shim `scripts/cesaropsctl.py` that detects Docker and runs the container, falling back to local run when Docker is unavailable.
