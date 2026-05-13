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

# ── Cloud / Hosting ───────────────────────────────────────────────────────────
# Used by: deploy_web.py, SFTP upload after scans

IONOS_SFTP_HOST=""                # IONOS SFTP hostname
IONOS_SFTP_USER=""                # IONOS SFTP username
IONOS_SFTP_PASS=""                # IONOS SFTP password
IONOS_SFTP_PATH="/wreckhunter/scans"

# ── Azure AI Services ─────────────────────────────────────────────────────────
# Used by: azure_vision_analyzer, BAG restoration analysis

AZURE_VISION_KEY=""               # Azure AI Vision API key
AZURE_VISION_ENDPOINT="https://wreckhunter2000.cognitiveservices.azure.com/"
AZURE_VISION_REGION="eastus"

# ── LLM Endpoints ────────────────────────────────────────────────────────────
# Used by: research daemon, Continue config, agent orchestrator

KOBOLD_URL="http://100.72.182.77:5001"
LLM_BASE_URL="http://100.72.182.77:5001/v1"
REASONING_BASE_URL="http://100.72.182.77:5001/v1"

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
