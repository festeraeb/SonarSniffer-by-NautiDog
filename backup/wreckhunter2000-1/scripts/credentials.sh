#!/bin/bash
# ══════════════════════════════════════════════════════════════════════════════
# CESAROPS Master Credentials File
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

EARTHDATA_TOKEN=""                # NASA Earthdata (Sentinel, Landsat, ICESat-2)
GOOGLE_CSE_KEY=""                 # Google Custom Search (research engine)
GOOGLE_CSE_CX=""                  # Google CSE engine ID
COPERNICUS_USER=""                # Copernicus Open Access Hub
COPERNICUS_PASS=""                # Copernicus password
CRATES_IO_TOKEN="ciocf58M5muXG38jMoYi471V06dGnDYR04Y" 
CLOUDFLARE_TOKEN="cfut_Z954D2n305Z1dU2JVKKHWON2NlD5afjfAnDlSKWm2323c625"
CLOUDFLARE_EMAIL="festeraeb@yahoo.com"
CLOUDFLARE_ZONE_ID="9739b87b018bcdc0ca8569f83c575801"
CLOUDFLARE_ACCOUNT_ID=""                        # (optional — grab from CF dashboard if needed)
# ── Cloud / Hosting (IONOS) ───────────────────────────────────────────────────
# Used by: deploy_web.py, SFTP upload after scans, ionos_ddns.py

IONOS_SFTP_HOST="access-5019147877.webspace-host.com"
IONOS_SFTP_USER="a1268970"
IONOS_SFTP_PASS="@Juliek01241973"
IONOS_SFTP_PORT="22"
IONOS_SFTP_PATH="/wreckhunter/scans"

# IONOS DNS API (for DDNS updates — home.cesarops.com A record)
IONOS_API_KEY=""                  # prefix.secret from https://developer.hosting.ionos.com
IONOS_ZONE="cesarops.com"
IONOS_RECORD_NAME="home"

# ── Azure AI Services ─────────────────────────────────────────────────────────
# Used by: azure_vision_analyzer, BAG restoration analysis

AZURE_VISION_KEY=""               # Azure AI Vision API key
AZURE_VISION_ENDPOINT="https://wreckhunter2000.cognitiveservices.azure.com/"
AZURE_VISION_REGION="eastus"

# ── LLM Endpoints ────────────────────────────────────────────────────────────
# Used by: research daemon, Continue config, agent orchestrator

KOBOLD_URL="https://llm.cesarops.org"
LLM_BASE_URL="https://llm.cesarops.org/v1"
REASONING_BASE_URL="https://llm.cesarops.org/v1"

# ── Tailscale ─────────────────────────────────────────────────────────────────
# Node IPs (for reference — not secrets, but handy to have here)

TS_T440="100.72.182.77"
TS_CESAROPS2="100.102.158.111"
TS_CESAROPS3="100.105.77.74"
TS_PI="100.127.66.32"

# ── Helper Function ───────────────────────────────────────────────────────────
# Source this file then use run_sudo for passwordless automation

run_sudo() {
    echo "$SUDO_PASS" | sudo -S "$@" 2>/dev/null
}

# Machine-specific sudo helpers
run_sudo_t440() {
    echo "$SUDO_PASS_T440" | sudo -S "$@" 2>/dev/null
}
