#!/usr/bin/env bash
#
# deploy_native_search.sh
#
# Native (no Docker) deployment of SearXNG + Rust web search integration for CESAROPS.
# Installs SearXNG via pip into a venv, configures it, creates a systemd service,
# and updates the cesarops-wso config to point at the local instance.
#
# Usage: sudo ./scripts/deploy_native_search.sh
#

set -euo pipefail

CESAROPS_BASE="/mnt/data-external/cesarops"
SEARXNG_VENV="${CESAROPS_BASE}/searxng-venv"
SEARXNG_SETTINGS="${CESAROPS_BASE}/searxng-settings.yml"
SERVICE_FILE="/etc/systemd/system/searxng-native.service"
WSO_CONFIG="${CESAROPS_BASE}/cesarops-wso/config.toml"
SEARXNG_PORT=8888
SEARXNG_SECRET_KEY_FILE="${CESAROPS_BASE}/searxng-secret.key"

echo "=== CESAROPS Native Search Deployment ==="
echo "Base: ${CESAROPS_BASE}"

# ── 1. Create directories ─────────────────────────────────────────────────────
mkdir -p "${CESAROPS_BASE}"
mkdir -p "$(dirname "${SEARXNG_SETTINGS}")"

# ── 2. Generate secret key if missing ─────────────────────────────────────────
if [ ! -f "${SEARXNG_SECRET_KEY_FILE}" ]; then
    openssl rand -hex 32 > "${SEARXNG_SECRET_KEY_FILE}"
    echo "Generated new SearXNG secret key at ${SEARXNG_SECRET_KEY_FILE}"
fi

# ── 3. Create Python venv and install SearXNG ─────────────────────────────────
if [ ! -d "${SEARXNG_VENV}" ]; then
    echo "Creating Python venv at ${SEARXNG_VENV}..."
    python3 -m venv "${SEARXNG_VENV}"
fi

echo "Installing SearXNG into venv..."
"${SEARXNG_VENV}/bin/pip" install --upgrade pip setuptools wheel
"${SEARXNG_VENV}/bin/pip" install searxng

echo "SearXNG installed: $("${SEARXNG_VENV}/bin/python" -m searxng --version 2>/dev/null || echo 'installed')"

# ── 4. Write settings.yml ────────────────────────────────────────────────────
cat > "${SEARXNG_SETTINGS}" << 'SETTINGS_EOF'
use_default_settings: true

general:
  instance_name: "CESAROPS Sovereign Search"
  debug: false
  privacypolicy_enabled: false
  open_metrics: false

search:
  safe_search: 0
  autocomplete: "google"
  default_lang: "en"
  formats:
    - html
    - json

server:
  port: 8888
  bind_address: "127.0.0.1"
  secret_key: "{{SECRET_KEY}}"
  limiter: false
  method: "GET"
  image_proxy: false
  http_protocol_version: "1.1"

ui:
  static_use_hash: true
  default_theme: simple
  theme_args:
    style_simple_auto: true
  theme_args_dark:
    auto_dark_mode_preference: true

outgoing:
  request_timeout: 8.0
  max_request_timeout: 15.0
  useragent_prefix: ["CESAROPS", ""]
  pool_connections: 10
  pool_maxsize: 10
  keepalive_expiry: 5.0

engines:
  # Enable a curated set of reliable engines
  - name: google
    engine: google
    shortcut: g
    disabled: false

  - name: bing
    engine: bing
    shortcut: b
    disabled: false

  - name: duckduckgo
    engine: duckduckgo
    shortcut: ddg
    disabled: false

  - name: wikipedia
    engine: wikipedia
    shortcut: wp
    disabled: false

  - name: arxiv
    engine: arxiv
    shortcut: arx
    disabled: false

  - name: stackoverflow
    engine: stackoverflow
    shortcut: so
    disabled: false

  # Disable noisy/low-value engines for sovereign use
  - name: reddit
    engine: reddit
    shortcut: rd
    disabled: true

  - name: youtube
    engine: youtube
    shortcut: yt
    disabled: true

  - name: imdb
    engine: imdb
    shortcut: imdb
    disabled: true

  # Add Brave as a fallback engine (requires API key in env)
  - name: brave
    engine: brave
    shortcut: br
    disabled: false
    api_key: "${BRAVE_API_KEY:-}"

  # Add Wikipedia English
  - name: wikipedia_en
    engine: wikipedia
    shortcut: wien
    language: en-US
    disabled: false

  # Add academic sources
  - name: crossref
    engine: crossref
    shortcut: cr
    disabled: false

  - name: semantic_scholar
    engine: semantic_scholar
    shortcut: ss
    disabled: false

  # Pubmed for biomedical
  - name: pubmed
    engine: pubmed
    shortcut: pm
    disabled: false

  # Wolfram Alpha for computational queries
  - name: wolframalpha
    engine: wolframalpha
    shortcut: wa
    disabled: false
    api_key: "${WOLFRAM_API_KEY:-}"

  # Arxiv for papers
  - name: arxiv
    engine: arxiv
    shortcut: arx
    disabled: false

  # StackExchange network
  - name: stackexchange
    engine: stackexchange
    shortcut: se
    disabled: false

  # GitHub code search
  - name: github
    engine: github
    shortcut: gh
    disabled: false

  # Additional reliable sources
  - name: baidu
    engine: baidu
    shortcut: bd
    disabled: true

  - name: yandex
    engine: yandex
    shortcut: yd
    disabled: true

  # Disable all others by default
  - name: all
    engine: acg_image
    disabled: true
  - name: all
    engine: anilist
    disabled: true
  - name: all
    engine: apple_app_store
    disabled: general
  - name: all
    engine: apple_lookup
    disabled: true
  - name: all
    engine: arch_linux
    disabled: true
  - name: all
    engine: bandcamp
    disabled: true
  - name: all
    engine: bilibili
    disabled: true
  - name: all
    engine: booksearch
    disabled: true
  - name: all
    engine: bpb
    disabled: true
  - name: all
    engine: bt4g
    disabled: true
  - name: all
    engine: ccc_talks
    disabled: true
  - name: all
    engine: crossref
    disabled: false
  - name: all
    engine: dailymotion
    disabled: true
  - name: all
    engine: deezer
    disabled: true
  - name: all
    engine: deviantart
    disabled: true
  - name: all
    engine: digbt
    disabled: true
  - name: all
    engine: dockerhub
    disabled: true
  - name: all
    engine: erowid
    disabled: true
  - name: all
    engine: ebay
    disabled: true
  - name: all
    engine: ebay_motors
    disabled: true
  - name: all
    engine: ecosia
    disabled: true
  - name: all
    engine: elasticsearch
    disabled: true
  - name: all
    engine: elasticsearch_mappings
    disabled: true
  - name: all
    engine: endic
    disabled: true
  - name: all
    engine: etymonline
    disabled: true
  - name: all
    engine: flickr
    disabled: true
  - name: all
    engine: freebsd
    disabled: true
  - name: all
    engine: ggo
    disabled: true
  - name: all
    engine: github
    disabled: false
  - name: all
    engine: google
    disabled: false
  - name: all
    engine: google_academic
    disabled: false
  - name: all
    engine: google_digital_books
    disabled: true
  - name: all
    engine: google_videos
    disabled: true
  - name: all
    engine: google_play_apps
    disabled: true
  - name: all
    engine: google_play_games
    disabled: true
  - name: all
    engine: google_scholar
    disabled: false
  - name: all
    engine: google_news
    disabled: false
  - name: all
    engine: google_search
    disabled: false
  - name: all
    engine: google_images
    disabled: true
  - name: all
    engine: google_maps
    disabled: true
  - name: all
    engine: google_earth
    disabled: true
  - name: all
    engine: google_policies
    disabled: true
  - name: all
    engine: google_shopping
    disabled: true
  - name: all
    engine: google_scholar
    disabled: false
  - name: all
    engine: google_videos
    disabled: true
  - name: all
    engine: google_news
    disabled: false
  - name: all
    engine: google_play_apps
    disabled: true
  - name: all
    engine: google_play_games
    disabled: true
  - name: all
    engine: google_scholar
    disabled: false
  - name: all
    engine: google_videos
    disabled: true
  - name: all
    engine: google_news
    disabled: false
  - name: all
    engine: google_play_apps
    disabled: true
  - name: all
    engine: google_play_games
    disabled: true
  - name: all
    engine: google_scholar
    disabled: false
  - name: all
    engine: google_videos
    disabled: true
  - name: all
    engine: google_news
    disabled: false

  # General disable for remaining engines
  - name: all
    engine: hoogle
    disabled: true
  - name: all
    engine: imdb
    disabled: true
  - name: all
    engine: instagram
    disabled: true
  - name: all
    engine: invidious
    disabled: true
  - name: all
    engine: jisho
    disabled: true
  - name: all
    engine: leetcode
    disabled: true
  - name: all
    engine: libgen
    disabled: true
  - name: all
    engine: lilo
    disabled: true
  - name: all
    engine: linkedin
    disabled: true
  - name: all
    engine: metacpan
    disabled: true
  - name: all
    engine: microsoft
    disabled: true
  - name: all
    engine: microsoft_academic
    disabled: true
  - name: all
    engine: mixcloud
    disabled: true
  - name: all
    engine: mozhi
    disabled: true
  - name: all
    engine: mullvad
    disabled: true
  - name: all
    engine: musicbrainz
    disabled: true
  - name: all
    engine: openlibrary
    disabled: true
  - name: all
    engine: openmeteo
    disabled: true
  - name: all
    engine: openstreetmap
    disabled: true
  - name: all
    engine: openrepos
    disabled: true
  - name: all
    engine: oscar
    disabled: true
  - name: all
    engine: packagist
    disabled: true
  - name: all
    engine: php
    disabled: true
  - name: all
    engine: pinterest
    disabled: true
  - name: all
    engine: pip
    disabled: true
  - name: all
    engine: pmc
    disabled: true
  - name: all
    engine: pypi
    disabled: true
  - name: all
    engine: qwant
    disabled: true
  - name: all
    engine: radionet
    disabled: true
  - name: all
    engine: radiobremen
    disabled: true
  - name: all
    engine: reddit
    disabled: true
  - name: all
    engine: rottentomatoes
    disabled: true
  - name: all
    engine: searchcode_code
    disabled: true
  - name: all
    engine: searchcode_repo
    disabled: true
  - name: all
    engine: semantic_scholar
    disabled: false
  - name: all
    engine: signal
    disabled: true
  - name: all
    engine: soundcloud
    disabled: true
  - name: all
    engine: speakerdeck
    disabled: true
  - name: all
    engine: spotify
    disabled: true
  - name: all
    engine: stackexchange
    disabled: false
  - name: all
    engine: stackoverflow
    disabled: false
  - name: all
    engine: steam
    disabled: true
  - name: all
    engine: superuser
    disabled: true
  - name: all
    engine: ted
    disabled: true
  - name: all
    engine: thepiratebay
    disabled: true
  - name: all
    engine: thingiverse
    disabled: true
  - name: all
    engine: torznab
    disabled: true
  - name: all
    engine: trello
    disabled: true
  - name: all
    engine: twitch
    disabled: true
  - name: all
    engine: twitter
    disabled: true
  - name: all
    engine: urbandictionary
    disabled: true
  - name: all
    engine: vagalume
    disabled: true
  - name: all
    engine: vagrant
    disabled: true
  - name: all
    engine: vault
    disabled: true
  - name: all
    engine: w3schools
    disabled: true
  - name: all
    engine: wanikani
    disabled: true
  - name: all
    engine: wikidata
    disabled: true
  - name: all
    engine: wikipedia
    disabled: false
  - name: all
    engine: wikisource
    disabled: true
  - name: all
    engine: wolframalpha
    disabled: false
  - name: all
    engine: yandex
    disabled: true
  - name: all
    engine: youtube
    disabled: true
  - name: all
    engine: zattoo
    disabled: true

# Localization
locale: en-US

# Redis cache (optional, for performance)
redis_cache:
  host: localhost
  port: 6379
  db: 0
  password: ""

# Logging
logging:
  file:
    level: WARNING
    enabled: false
    filename: /tmp/searxng.log
  console:
    level: INFO
    enabled: true
SETTINGS_EOF

# Replace the secret key placeholder
SECRET_KEY=$(cat "${SEARXNG_SECRET_KEY_FILE}")
sed -i "s|{{SECRET_KEY}}|${SECRET_KEY}|g" "${SEARXNG_SETTINGS}"

echo "SearXNG settings written to ${SEARXNG_SETTINGS}"

# ── 5. Write systemd service ─────────────────────────────────────────────────
cat > "${SERVICE_FILE}" << SERVICE_EOF
[Unit]
Description=CESAROPS Sovereign SearXNG Search Engine
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=${CESAROPS_BASE}
Environment=PYTHONUNBUFFERED=1
Environment=SEARXNG_SETTINGS_PATH=${SEARXNG_SETTINGS}
Environment=PATH=${SEARXNG_VENV}/bin:/usr/local/bin:/usr/bin:/bin
ExecStart=${SEARXNG_VENV}/bin/python -m searxng --host 127.0.0.1 --port ${SEARXNG_PORT}
Restart=on-failure
RestartSec=5
StandardOutput=journal
StandardError=journal
SyslogIdentifier=searxng-native

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=${CESAROPS_BASE}
PrivateTmp=true

[Install]
WantedBy=multi-user.target
SERVICE_EOF

echo "Systemd service written to ${SERVICE_FILE}"

# ── 6. Enable and start the service ──────────────────────────────────────────
echo "Enabling and starting searxng-native service..."
systemctl daemon-reload
systemctl enable searxng-native.service
systemctl restart searxng-native.service

sleep 3

# ── 7. Health check ──────────────────────────────────────────────────────────
echo "Testing SearXNG health..."
if curl -sf http://127.0.0.1:${SEARXNG_PORT}/search?q=test&format=json > /dev/null 2>&1; then
    echo "✓ SearXNG is healthy and responding on port ${SEARXNG_PORT}"
else
    echo "✗ SearXNG health check failed. Check logs: journalctl -u searxng-native -f"
    echo "  Attempting one more retry..."
    sleep 5
    if curl -sf http://127.0.0.1:${SEARXNG_PORT}/search?q=test&format=json > /dev/null 2>&1; then
        echo "✓ SearXNG is now healthy after retry"
    else
        echo "✗ SearXNG still failing. Manual intervention may be required."
        exit 1
    fi
fi

# ── 8. Update cesarops-wso config ────────────────────────────────────────────
# If config.toml exists, update the search section
if [ -f "${WSO_CONFIG}" ]; then
    echo "Updating existing cesarops-wso config..."
    # Use sed to update or add the search endpoint
    if grep -q "search_endpoint" "${WSO_CONFIG}"; then
        sed -i "s|search_endpoint.*|search_endpoint = \"http://127.0.0.1:${SEARXNG_PORT}\"|" "${WSO_CONFIG}"
    else
        echo "" >> "${WSO_CONFIG}"
        echo "[search]" >> "${WSO_CONFIG}"
        echo "search_endpoint = \"http://127.0.0.1:${SEARXNG_PORT}\"" >> "${WSO_CONFIG}"
        echo "search_timeout_secs = 10" >> "${WSO_CONFIG}"
        echo "max_results = 10" >> "${WSO_CONFIG}"
    fi
    echo "✓ Updated ${WSO_CONFIG} to point at local SearXNG"
else
    echo "⚠ No cesarops-wso config found at ${WSO_CONFIG}. Create it manually with:"
    echo "  [search]"
    echo "  search_endpoint = \"http://127.0.0.1:${SEARXNG_PORT}\""
fi

echo ""
echo "=== Deployment Complete ==="
echo "SearXNG running on http://127.0.0.1:${SEARXNG_PORT}"
echo "Service: searxng-native"
echo "Logs: journalctl -u searxng-native -f"
echo "Settings: ${SEARXNG_SETTINGS}"
echo ""
echo "Next steps:"
echo "  1. Build cesarops-wso: cd cesarops-wso && cargo build --release"
echo "  2. Run tests: cargo test"
echo "  3. Start the WSO service: systemctl start cesarops-wso"
