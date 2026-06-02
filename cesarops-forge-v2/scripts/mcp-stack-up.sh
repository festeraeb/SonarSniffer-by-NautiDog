#!/usr/bin/env bash
# Bring up Forge MCP sidecars without docker compose (plain docker run).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
ENV_FILE="${HOME}/.config/cesarops/mcp-stack.env"
OPENAI_KEY="${OPENAI_API_KEY:-none}"

mkdir -p "$(dirname "$ENV_FILE")"
if [[ ! -f "$ENV_FILE" ]]; then
  cat >"$ENV_FILE" <<'EOF'
# Optional: real OpenAI key enables OpenMemory fact extraction (container starts without it).
# OPENAI_API_KEY=sk-...
# CONTEXT7_API_KEY=...   # for MCP clients via http://127.0.0.1:3737/mcp
EOF
fi
if [[ -f "$ENV_FILE" ]]; then
  set -a
  # shellcheck disable=SC1090
  source "$ENV_FILE"
  set +a
  OPENAI_KEY="${OPENAI_API_KEY:-none}"
fi

run_or_replace() {
  local name="$1"
  shift
  docker rm -f "$name" >/dev/null 2>&1 || true
  docker run -d --name "$name" "$@"
}

# Context7: local reverse proxy to hosted MCP (no Upstash Redis required on-box).
run_or_replace context7-proxy \
  --restart unless-stopped \
  -p 127.0.0.1:3737:80 \
  -v "${ROOT}/docker/context7-proxy.conf:/etc/nginx/conf.d/default.conf:ro" \
  nginx:alpine

# Crawl4AI REST API
run_or_replace crawl4ai \
  --restart unless-stopped \
  -p 127.0.0.1:11235:11235 \
  --shm-size=1g \
  unclecode/crawl4ai:latest

# OpenMemory MCP (uses host Qdrant on :6333)
run_or_replace openmemory-mcp \
  --restart unless-stopped \
  -p 127.0.0.1:8765:8765 \
  --add-host=host.docker.internal:host-gateway \
  -e "USER=${USER:-cesarops}" \
  -e QDRANT_HOST=host.docker.internal \
  -e QDRANT_PORT=6333 \
  -e "OPENAI_API_KEY=${OPENAI_KEY}" \
  mem0/openmemory-mcp:latest

echo "MCP stack:"
docker ps --filter name='context7-proxy|crawl4ai|openmemory-mcp' --format 'table {{.Names}}\t{{.Status}}\t{{.Ports}}'
