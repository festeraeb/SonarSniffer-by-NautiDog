#!/bin/bash
# ══════════════════════════════════════════════════════════════════════════════
# CESAROPS Master Credentials File

# Earthdata / satellite: canonical on T440 is repo .env (also /data/cesarops/repo/.env).
# Optional mirrors: ~/.ssh/credentials.ssh, ~/.ssh/credentials (KEY=VALUE).
_REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
for _envf in \
  "${HOME}/.ssh/credentials.ssh" \
  "${HOME}/.ssh/credentials" \
  "/mnt/data-external/cesarops/repo/.env" \
  "/data/cesarops/repo/.env" \
  "$_REPO_ROOT/.env"; do
  if [[ -f "$_envf" ]]; then
    set -a
    # shellcheck disable=SC1090
    source "$_envf"
    set +a
  fi
done

# Fallback Earthdata token files (used by several legacy pipelines)
if [[ -z "${EARTHDATA_TOKEN:-}" ]]; then
  for _tokf in \
    "$_REPO_ROOT/backup/deploy/detection/sentinel_hunt/earthdata_token.json" \
    "$_REPO_ROOT/backup/wreckhunter2000-1/sentinel_hunt_src/earthdata_token.json" \
    "$_REPO_ROOT/pipelines/erie_remote/erie_remote_data/.earthdata_token"; do
    if [[ -f "$_tokf" ]]; then
      _tok="$(python3 - <<'PY' "$_tokf" 2>/dev/null
import json, sys
p = sys.argv[1]
try:
    t = open(p, encoding='utf-8').read().strip()
    if t.startswith('{'):
        d = json.loads(t)
        print((d.get('earthdata_token') or '').strip())
    else:
        print(t)
except Exception:
    pass
PY
)"
      if [[ -n "$_tok" ]]; then
        EARTHDATA_TOKEN="$_tok"
        break
      fi
    fi
  done
fi
unset _envf
# _REPO_ROOT kept for gemini local creds block below
# ══════════════════════════════════════════════════════════════════════════════
# All authentication tokens, passwords, and API keys in ONE place.
# Sourced by automation scripts across the cluster.
#
# NEVER commit this file to git — it's in .gitignore.
# Copy to each machine and update machine-specific values.
# ══════════════════════════════════════════════════════════════════════════════

# ── Cluster Sudo Passwords ────────────────────────────────────────────────────
# Used by: swap_model.sh, deploy_services.sh, any script needing systemctl

SUDO_PASS_T440="cesarops"                       # T440 conductor (100.72.182.77)
SUDO_PASS_CESAROPS2="cesarops"                  # cesarops2 (100.102.158.111)
SUDO_PASS_CESAROPS3="cesarops"                  # cesarops3 (100.105.77.74)

# Default — used when script doesn't specify which machine
SUDO_PASS="$SUDO_PASS_T440"

# ── API Tokens ────────────────────────────────────────────────────────────────
# Used by: research daemon, satellite downloaders, sensor pipelines

# NASA Earthdata — repo .env sourced above; fallbacks if missing on a node
: "${EARTHDATA_USERNAME:=}"
: "${EARTHDATA_PASSWORD:=}"
: "${EARTHDATA_TOKEN:=}"
: "${NASA_EARTHDATA_TOKEN:=$EARTHDATA_TOKEN}"
: "${GOOGLE_CSE_KEY:=}"
: "${GOOGLE_CSE_CX:=}"
: "${COPERNICUS_USER:=}"
: "${COPERNICUS_PASS:=}"
: "${COPERNICUS_USERNAME:=$COPERNICUS_USER}"
: "${COPERNICUS_PASSWORD:=$COPERNICUS_PASS}"
: "${ASF_USERNAME:=$EARTHDATA_USERNAME}"
: "${ASF_PASSWORD:=}"
: "${ASF_TOKEN:=$EARTHDATA_TOKEN}"
: "${ASF_API_TOKEN:=$ASF_TOKEN}"
: "${ASF_USER:=$ASF_USERNAME}"
: "${ASF_PASS:=$ASF_PASSWORD}"

# CEOS FedEO (open discovery interfaces; optional creds depending on downstream service)
: "${FEDEO_BASE_URL:=https://fedeo.ceos.org}"
: "${FEDEO_API_URL:=$FEDEO_BASE_URL/api}"
: "${FEDEO_STAC_URL:=$FEDEO_BASE_URL/}"
: "${FEDEO_OPENSEARCH_DESCRIPTION_URL:=$FEDEO_API_URL?httpAccept=application%2Fopensearchdescription%2Bxml}"
: "${FEDEO_EXPLAIN_URL:=$FEDEO_API_URL?httpAccept=application%2Fjson%3Bprofile%3D%22http%3A%2F%2Fexplain.z3950.org%2Fdtd%2F2.0%2F%22}"
: "${FEDEO_OPENAPI_URL:=https://fedeo.ceos.org/api}"
: "${FEDEO_USERNAME:=}"
: "${FEDEO_PASSWORD:=}"

export EARTHDATA_USERNAME EARTHDATA_PASSWORD EARTHDATA_TOKEN NASA_EARTHDATA_TOKEN
export GOOGLE_CSE_KEY GOOGLE_CSE_CX COPERNICUS_USER COPERNICUS_PASS COPERNICUS_USERNAME COPERNICUS_PASSWORD
export ASF_USERNAME ASF_PASSWORD ASF_TOKEN ASF_API_TOKEN ASF_USER ASF_PASS
export FEDEO_BASE_URL FEDEO_API_URL FEDEO_STAC_URL FEDEO_OPENSEARCH_DESCRIPTION_URL FEDEO_EXPLAIN_URL FEDEO_OPENAPI_URL FEDEO_USERNAME FEDEO_PASSWORD

# ── USGS M2M API Token ───────────────────────────────────────────────────────
# Application token from https://ers.cr.usgs.gov/profile
# 64-bit encrypted string for M2M API authentication.
# login-token endpoint requires BOTH the ERS username and the token.
USGS_M2M_USERNAME="cesarops"
USGS_M2M_TOKEN="nK0ModqZD3ZpJeQvy!2Ku6Ogr@trroZRtr4v7NjnGvxp9nf217w4tomIgx2VGT7I"
USGS_API_KEY="$USGS_M2M_TOKEN"
USGS_M2M_API_KEY="$USGS_M2M_TOKEN"
export USGS_M2M_USERNAME USGS_M2M_TOKEN USGS_API_KEY USGS_M2M_API_KEY

# ── Other API Tokens ─────────────────────────────────────────────────────────

# Google Gemini — edit scripts/credentials.gemini.local.sh (small file; gitignored)
_GEMINI_CRED="${_REPO_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}/scripts/credentials.gemini.local.sh"
if [[ -f "$_GEMINI_CRED" ]]; then
  # shellcheck disable=SC1090
  source "$_GEMINI_CRED"
fi
: "${GEMINI_API_KEY:=}"
: "${GEMINI_MODEL:=gemini-2.5-flash}"
export GEMINI_API_KEY GEMINI_MODEL
unset _GEMINI_CRED

CRATES_IO_TOKEN="ciocf58M5muXG38jMoYi471V06dGnDYR04Y"
CLOUDFLARE_TOKEN="cfut_Z954D2n305Z1dU2JVKKHWON2NlD5afjfAnDlSKWm2323c625"
CLOUDFLARE_EMAIL="festeraeb@yahoo.com"
CLOUDFLARE_ZONE_ID="9739b87b018bcdc0ca8569f83c575801"
CLOUDFLARE_ACCOUNT_ID=""

GITHUB_PAT="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"

# ── Cluster LAN hosts (10.0.0.x — all nodes share this LAN) ───────────────────
# Restored host block. Prefer LAN (10.0.0.x) over Tailscale (100.x) for fleet
# work — faster, and the old 100.x cesarops2/3 devices are stale/down.
SSH_USER="cesarops"

# T440 conductor — dual-homed (two NICs on the same box).
LAN_T440="10.0.0.61"
LAN_T440_ALT="10.0.0.62"

# ML350e (hostname cesarops2) — dual-homed (two NICs on the same box).
LAN_CESAROPS2="10.0.0.200"
LAN_CESAROPS2_ALT="10.0.0.201"

# Laptop worker — set when it joins the LAN (currently offline).
LAN_LAPTOP="${LAN_LAPTOP:-}"

# Shared roomy data root (998 GB free on /data; /codebase is tight at ~58 GB).
CESAROPS_SHARED_DATA="${CESAROPS_SHARED_DATA:-/data/cesarops/satellite_data}"

export SSH_USER LAN_T440 LAN_T440_ALT LAN_CESAROPS2 LAN_CESAROPS2_ALT LAN_LAPTOP CESAROPS_SHARED_DATA
