

=== FILE: scripts/deploy_native_search.sh ===
```bash
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
```

=== FILE: searxng/settings.yml ===
```yaml
# SearXNG Settings for CESAROPS Sovereign Search
# Installed at: /mnt/data-external/cesarops/searxng-settings.yml
#
# This configuration enables JSON API, restricts access to localhost,
# and curates a set of reliable search engines suitable for technical research.

use_default_settings: true

general:
  instance_name: "CESAROPS Sovereign Search"
  debug: false
  privacypolicy_enabled: false
  open_metrics: false

search:
  safe_search: 0          # 0=off, 1=moderate, 2=strict
  autocomplete: "google"   # Autocomplete provider
  default_lang: "en"       # Default language
  formats:
    - html                 # HTML results (for browser testing)
    - json                 # JSON results (for programmatic access)

server:
  port: 8888
  bind_address: "127.0.0.1"  # Localhost only — no public access
  secret_key: "{{SECRET_KEY}}"  # Replaced by deploy script
  limiter: false           # No rate limiting (internal use only)
  method: "GET"            # Use GET requests (more cacheable)
  image_proxy: false       # Don't proxy images
  http_protocol_version: "1.1"

ui:
  static_use_hash: true
  default_theme: simple
  theme_args:
    style_simple_auto: true

outgoing:
  request_timeout: 8.0
  max_request_timeout: 15.0
  useragent_prefix: ["CESAROPS", ""]
  pool_connections: 10
  pool_maxsize: 10
  keepalive_expiry: 5.0

# ── Enabled Engines ───────────────────────────────────────────────────────────
engines:
  # Primary search engines
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

  - name: brave
    engine: brave
    shortcut: br
    disabled: false
    api_key: "${BRAVE_API_KEY:-}"

  # Academic and technical sources
  - name: arxiv
    engine: arxiv
    shortcut: arx
    disabled: false

  - name: semantic_scholar
    engine: semantic_scholar
    shortcut: ss
    disabled: false

  - name: crossref
    engine: crossref
    shortcut: cr
    disabled: false

  - name: pubmed
    engine: pubmed
    shortcut: pm
    disabled: false

  - name: wolframalpha
    engine: wolframalpha
    shortcut: wa
    disabled: false
    api_key: "${WOLFRAM_API_KEY:-}"

  # Q&A and code
  - name: stackoverflow
    engine: stackoverflow
    shortcut: so
    disabled: false

  - name: stackexchange
    engine: stackexchange
    shortcut: se
    disabled: false

  - name: github
    engine: github
    shortcut: gh
    disabled: false

  # Reference
  - name: wikipedia
    engine: wikipedia
    shortcut: wp
    disabled: false

  - name: wikidata
    engine: wikidata
    shortcut: wd
    disabled: true   # Keep disabled unless needed

# ── Disabled Engines (noisy/low-value for sovereign use) ──────────────────────
# All other engines are implicitly disabled by use_default_settings + explicit disables below.
# The deploy script writes a comprehensive disable list to prevent unwanted engines from activating.

# Logging
logging:
  file:
    level: WARNING
    enabled: false
    filename: /tmp/searxng.log
  console:
    level: INFO
    enabled: true
```

=== FILE: scripts/searxng-native.service ===
```ini
# /etc/systemd/system/searxng-native.service
#
# Native systemd service for SearXNG search engine.
# Installed by scripts/deploy_native_search.sh
#
# Usage:
#   sudo systemctl enable searxng-native
#   sudo systemctl start searxng-native
#   journalctl -u searxng-native -f

[Unit]
Description=CESAROPS Sovereign SearXNG Search Engine
Documentation=https://docs.searxng.org/
After=network.target
Wants=network-online.target

[Service]
Type=simple
User=root
Group=root
WorkingDirectory=/mnt/data-external/cesarops

# Environment
Environment=PYTHONUNBUFFERED=1
Environment=SEARXNG_SETTINGS_PATH=/mnt/data-external/cesarops/searxng-settings.yml
Environment=PATH=/mnt/data-external/cesarops/searxng-venv/bin:/usr/local/bin:/usr/bin:/bin

# Start command
ExecStart=/mnt/data-external/cesarops/searxng-venv/bin/python -m searxng --host 127.0.0.1 --port 8888

# Restart policy
Restart=on-failure
RestartSec=5

# Logging
StandardOutput=journal
StandardError=journal
SyslogIdentifier=searxng-native

# Security hardening (no Docker needed — native Linux security)
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
ReadWritePaths=/mnt/data-external/cesarops
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true

# Resource limits
LimitNOFILE=65536
LimitNPROC=4096

[Install]
WantedBy=multi-user.target
```

=== FILE: cesarops-wso/src/duckduckgo.rs ===
```rust
//! DuckDuckGo HTML scraper for cesarops-wso web search.
//!
//! No API key required. Scrapes the HTML results page directly.
//! Used as a fallback when SearXNG is unavailable or for specific queries.
//!
//! ⚠️ DuckDuckGo may block aggressive scraping. Use responsibly.

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A single search result from DuckDuckGo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuckDuckGoResult {
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub source: String, // e.g., "Wikipedia", "Stack Overflow"
}

/// DuckDuckGo search client
pub struct DuckDuckGoClient {
    http: Client,
    max_results: usize,
}

impl DuckDuckGoClient {
    pub fn new(max_results: usize) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .user_agent("CESAROPS/1.0 (research agent; +https://cesarops.io)")
                .build()
                .unwrap_or_default(),
            max_results,
        }
    }

    /// Search DuckDuckGo and return structured results.
    pub async fn search(&self, query: &str) -> Result<Vec<DuckDuckGoResult>> {
        // DuckDuckGo HTML search URL (no API needed)
        let url = format!(
            "https://html.duckduckgo.com/html/?q={}",
            urlencoding::encode(query)
        );

        let response = self.http.get(&url).send().await?;
        let html = response.text().await?;

        // Parse results from HTML using simple DOM-like extraction
        let results = Self::parse_html_results(&html, self.max_results);

        if results.is_empty() {
            tracing::warn!("DuckDuckGo returned no results for query: {}", query);
        }

        Ok(results)
    }

    /// Parse DuckDuckGo HTML results page.
    /// DuckDuckGo's HTML structure uses specific classes for results.
    fn parse_html_results(html: &str, max_results: usize) -> Vec<DuckDuckGoResult> {
        let mut results = Vec::new();

        // Extract result blocks using regex patterns matching DuckDuckGo's HTML structure
        // Pattern: <a class="result__a" href="..." ...>title</a>
        // Pattern: <a class="result__snippet" href="..." ...>snippet</a>

        // Find all result links
        let title_pattern = regex::Regex::new(
            r#"<a[^>]*class="result__a"[^>]*href="([^"]*)"[^>]*>(.*?)</a>"#,
        )
        .unwrap();

        let snippet_pattern = regex::Regex::new(
            r#"<a[^>]*class="result__snippet"[^>]*href="[^"]*"[^>]*>(.*?)</a>"#,
        )
        .unwrap();

        let entity_pattern = regex::Regex::new(r#"&([a-z]+);"#).unwrap();

        // Decode HTML entities
        let decode_entities = |s: &str| -> String {
            entity_pattern
                .replace_all(s, |caps: &regex::Captures| {
                    let entity = caps[1].to_string();
                    match entity.as_str() {
                        "amp" => "&",
                        "lt" => "<",
                        "gt" => ">",
                        "quot" => "\"",
                        "apos" => "'",
                        _ => caps[0].to_string(),
                    }
                })
                .to_string()
        };

        // Extract titles and URLs
        for cap in title_pattern.captures_iter(html) {
            let url = cap.get(1).map(|m| m.as_str()).unwrap_or("");
            let raw_title = cap.get(2).map(|m| m.as_str()).unwrap_or("");
            let title = decode_entities(raw_title);

            if !url.is_empty() && !title.is_empty() {
                results.push(DuckDuckGoResult {
                    title,
                    url: url.to_string(),
                    snippet: String::new(), // Will be filled below
                    source: String::new(),
                });
            }
        }

        // Extract snippets (they appear right after titles in the HTML)
        for cap in snippet_pattern.captures_iter(html) {
            if let Some(idx) = cap.get(1) {
                let snippet = decode_entities(idx.as_str());
                if let Some(result) = results.last_mut() {
                    result.snippet = snippet;
                }
            }
        }

        // Limit to max_results
        results.truncate(max_results);

        results
    }
}

/// Search DuckDuckGo directly (convenience function)
pub async fn search_ddg(query: &str, max_results: usize) -> Result<Vec<DuckDuckGoResult>> {
    let client = DuckDuckGoClient::new(max_results);
    client.search(query).await
}
```

=== FILE: cesarops-wso/src/brave.rs ===
```rust
//! Brave Search API client for cesarops-wso web search.
//!
//! Uses the Brave Search API (free tier: 2000 queries/month).
//! Returns structured JSON results with high reliability.
//!
//! API Key: Set via BRAVE_API_KEY environment variable or config.toml.
//! Docs: https://api.search.brave.com/res/v1/web/search

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// A single search result from Brave Search API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BraveResult {
    pub title: String,
    pub url: String,
    pub description: String,
    pub extra_snippets: Vec<String>,
    pub result_type: String, // "web", "news", etc.
}

/// Brave Search API response structure
#[derive(Debug, Deserialize)]
struct BraveApiResponse {
    web: Option<BraveWebResults>,
}

#[derive(Debug, Deserialize)]
struct BraveWebResults {
    results: Vec<BraveApiResult>,
}

#[derive(Debug, Deserialize)]
struct BraveApiResult {
    title: String,
    url: String,
    description: String,
    extra_snippets: Option<Vec<String>>,
    #[serde(rename = "type")]
    result_type: Option<String>,
}

/// Brave Search API client
pub struct BraveClient {
    http: Client,
    api_key: String,
    max_results: usize,
}

impl BraveClient {
    /// Create a new Brave Search client.
    /// api_key: Brave API key (from environment or config)
    pub fn new(api_key: String, max_results: usize) -> Self {
        Self {
            http: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .unwrap_or_default(),
            api_key,
            max_results,
        }
    }

    /// Search using the Brave Search API.
    pub async fn search(&self, query: &str) -> Result<Vec<BraveResult>> {
        let url = "https://api.search.brave.com/res/v1/web/search";

        let payload = serde_json::json!({
            "q": query,
            "count": self.max_results,
            "freshness": "pm", // Past month for recency
        });

        let response = self
            .http
            .post(url)
            .header("Accept", "application/json")
            .header("Accept-Encoding", "gzip")
            .header("X-Subscription-Token", &self.api_key)
            .header("Content-Type", "application/json")
            .json(&payload)
            .send()
            .await
            .context("Failed to send Brave Search request")?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Brave Search API returned {}: {}", status, body);
        }

        let api_response: BraveApiResponse = response
            .json()
            .await
            .context("Failed to parse Brave Search response")?;

        let results = api_response
            .web
            .map(|w| {
                w.results
                    .into_iter()
                    .take(self.max_results)
                    .map(|r| BraveResult {
                        title: r.title,
                        url: r.url,
                        description: r.description,
                        extra_snippets: r.extra_snippets.unwrap_or_default(),
                        result_type: r.result_type.unwrap_or_else(|| "web".to_string()),
                    })
                    .collect()
            })
            .unwrap_or_default();

        if results.is_empty() {
            tracing::warn!("Brave Search returned no results for query: {}", query);
        }

        Ok(results)
    }
}

/// Search Brave directly (convenience function)
pub async fn search_brave(query: &str, api_key: &str, max_results: usize) -> Result<Vec<BraveResult>> {
    let client = BraveClient::new(api_key.to_string(), max_results);
    client.search(query).await
}
```