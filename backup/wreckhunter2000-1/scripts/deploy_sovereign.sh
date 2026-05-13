#!/usr/bin/env bash
# Deploy sovereign-cloud to the T440 (primary server, dual P100).
# i7 RETIRED (2026-05-06) — T440 is now the always-on server.
#
# Usage:
#   bash scripts/deploy_sovereign.sh              # sync + build + restart
#   bash scripts/deploy_sovereign.sh --sync-only  # push source only, no build
#   bash scripts/deploy_sovereign.sh --restart    # restart systemd service only
#   bash scripts/deploy_sovereign.sh --build-only # build + install, skip sync
#
# Reads from .env:
#   T440_HOST        LAN IP          (10.0.0.61)
#   T440_TAILSCALE   Tailscale/WAN   (100.72.182.77)  ← preferred when off-LAN
#   T440_USER        SSH username
#   T440_WORK        remote repo path

set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"

# ── Load .env ─────────────────────────────────────────────────────────────────
if [[ -f "$REPO/.env" ]]; then
    set -a; source "$REPO/.env"; set +a
fi

# Prefer Tailscale/WAN IP when set (works from laptop on any network)
HOST="${T440_TAILSCALE:-${T440_HOST:-10.0.0.61}}"
RUSER="${T440_USER:-cesarops}"
KEY="${I7_KEY:-}"
WORK="${T440_WORK:-/home/$RUSER/wreckhunter2000-1}"
# T440 runs its own LLM — but can also forward to XENON
XENON_LAN="${XENON_HOST:-10.0.0.129}"
LLM_URL="${LLM_BASE_URL:-${KOBOLD_BASE_URL:-http://localhost:5001/v1}}"
SSHPORT="${T440_SSH_PORT:-22}"
SVC_PORT=8765
DEPLOY_BIN="/opt/cesarops/sovereign-cloud"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -p $SSHPORT"
SCP_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -P $SSHPORT"
if [[ -n "$KEY" && -f "$KEY" ]]; then
    SSH_OPTS="$SSH_OPTS -i $KEY"
    SCP_OPTS="$SCP_OPTS -i $KEY"
fi

ssh_run() { ssh $SSH_OPTS "$RUSER@$HOST" "$@"; }

MODE="full"
case "${1:-}" in
    --sync-only)   MODE="sync"    ;;
    --restart)     MODE="restart"  ;;
    --build-only)  MODE="build"   ;;
    --setup-https) MODE="https"   ;;
esac

echo "═══════════════════════════════════════════════════════"
echo "  sovereign-cloud → $RUSER@$HOST:$SVC_PORT"
echo "  mode: $MODE   remote dir: $WORK"
echo "═══════════════════════════════════════════════════════"

# ── Connectivity ──────────────────────────────────────────────────────────────
echo ""
echo "[check] SSH connectivity..."
if ! ssh $SSH_OPTS "$RUSER@$HOST" true 2>/dev/null; then
    echo "[error] Cannot reach $HOST"
    echo "        Try: ssh $RUSER@$HOST"
    echo "        Or set up keys: ssh-copy-id -i ~/.ssh/id_rsa.pub $RUSER@$HOST"
    exit 1
fi
echo "[check] OK"

# ── Restart only ──────────────────────────────────────────────────────────────
if [[ "$MODE" == "restart" ]]; then
    ssh_run "sudo systemctl restart sovereign-cloud && sudo systemctl status sovereign-cloud --no-pager -l"
    exit 0
fi

# ── HTTPS setup only ──────────────────────────────────────────────────────────
if [[ "$MODE" == "https" ]]; then
    IONOS_KEY="${IONOS_API_KEY:-}"
    if [[ -z "$IONOS_KEY" ]]; then
        echo "[error] IONOS_API_KEY not set in .env"
        exit 1
    fi
    echo "[https] Copying setup_https_i7.sh to remote..."
    scp $SCP_OPTS "$REPO/scripts/setup_https_i7.sh" "$RUSER@$HOST:/tmp/setup_https_i7.sh"
    ssh_run "bash /tmp/setup_https_i7.sh '$IONOS_KEY'"
    exit 0
fi

# ── Sync source ───────────────────────────────────────────────────────────────
if [[ "$MODE" == "full" || "$MODE" == "sync" ]]; then
    echo ""
    echo "[sync] Checking for repo on remote..."

    REMOTE_HAS_REPO=$(ssh_run "[ -d '$WORK/.git' ] && echo yes || echo no")

    if [[ "$REMOTE_HAS_REPO" == "yes" ]]; then
        echo "[sync] Repo exists — git pull..."
        ssh_run "cd '$WORK' && git pull --ff-only"
    else
        echo "[sync] No repo found — rsyncing source..."
        ssh_run "mkdir -p '$WORK'"

        rsync -az --delete -e "ssh $SSH_OPTS" \
            "$REPO/sovereign-cloud/" "$RUSER@$HOST:$WORK/sovereign-cloud/"

        rsync -az --delete -e "ssh $SSH_OPTS" \
            "$REPO/nauticuvs/" "$RUSER@$HOST:$WORK/nauticuvs/"

        [[ -f "$REPO/Cargo.lock" ]] && \
            rsync -az -e "ssh $SSH_OPTS" "$REPO/Cargo.lock" "$RUSER@$HOST:$WORK/Cargo.lock"

        # Minimal workspace — only the two crates needed
        ssh_run "cat > '$WORK/Cargo.toml'" << 'TOML'
[workspace]
members = ["sovereign-cloud", "nauticuvs"]
resolver = "2"
TOML
        echo "[sync] Done."
    fi

    [[ "$MODE" == "sync" ]] && exit 0
fi

# ── Ensure Rust is installed ──────────────────────────────────────────────────
echo ""
echo "[rust] Checking Rust on remote..."
if ! ssh_run "source \$HOME/.cargo/env 2>/dev/null; command -v cargo > /dev/null 2>&1"; then
    echo "[rust] Not found — installing via rustup..."
    ssh_run "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path"
fi
echo "[rust] OK"

# ── Build dependencies ────────────────────────────────────────────────────────
echo ""
echo "[deps] Checking build tools..."
ssh_run "
    if ! command -v gcc > /dev/null 2>&1; then
        echo 'Installing build-essential...'
        sudo apt-get update -qq
        sudo apt-get install -y --no-install-recommends \
            build-essential pkg-config libssl-dev
    else
        echo 'gcc present'
    fi
"

# ── Build ─────────────────────────────────────────────────────────────────────
echo ""
echo "[build] Building sovereign-cloud --release on remote..."
echo "        (First run: ~10–15 min while compiling wgpu / nvml / axum)"
ssh_run "
    source \$HOME/.cargo/env
    cd '$WORK'
    cargo build --release -p sovereign-cloud
"
echo "[build] Done."

# ── Install binary ────────────────────────────────────────────────────────────
echo ""
echo "[install] Copying binary to $DEPLOY_BIN..."
ssh_run "
    sudo mkdir -p /opt/cesarops
    sudo cp '$WORK/target/release/sovereign-cloud' $DEPLOY_BIN
    sudo chmod +x $DEPLOY_BIN
    echo 'Installed.'
"

# ── Systemd service ───────────────────────────────────────────────────────────
echo ""
echo "[service] Installing/updating sovereign-cloud.service..."
ssh_run "sudo tee /etc/systemd/system/sovereign-cloud.service > /dev/null" << EOF
[Unit]
Description=CESARops sovereign-cloud node API
After=network.target ollama.service
Wants=ollama.service

[Service]
ExecStart=$DEPLOY_BIN
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
Environment="LLM_BASE_URL=$LLM_URL"
Environment="API_BIND_ADDR=127.0.0.1"
Environment="I7_HOST=${I7_HOST:-10.0.0.56}"
Environment="I7_TAILSCALE=${I7_TAILSCALE:-100.85.138.4}"
Environment="XENON_HOST=${XENON_HOST:-10.0.0.129}"
Environment="P1000_HOST=${P1000_HOST:-10.0.0.204}"
Environment="P1000_TAILSCALE=${P1000_TAILSCALE:-100.105.77.74}"
Environment="PI_HOST=${PI_HOST:-10.0.0.226}"
Environment="PI_TAILSCALE=${PI_TAILSCALE:-100.127.66.32}"
Environment="XBOX_HOST=${XBOX_HOST:-10.0.0.100}"
Environment="T440_HOST=${T440_HOST:-10.0.0.61}"
Environment="T440_TAILSCALE=${T440_TAILSCALE:-100.72.182.77}"
# TPU_SERVER_URL intentionally unset — this node IS the TPU server

[Install]
WantedBy=multi-user.target
EOF

ssh_run "
    sudo systemctl daemon-reload
    sudo systemctl enable sovereign-cloud
    sudo systemctl restart sovereign-cloud
    sleep 2
    sudo systemctl status sovereign-cloud --no-pager -l
"

# ── Health check ──────────────────────────────────────────────────────────────
echo ""
echo "[health] Checking API..."
sleep 2
if ssh_run "curl -sf http://localhost:$SVC_PORT/health"; then
    echo ""
    echo "═══════════════════════════════════════════════════════"
    echo "  ✓  sovereign-cloud is live"
    echo ""
    echo "     http://$HOST:$SVC_PORT/health"
    echo "     http://$HOST:$SVC_PORT/v1/node/status"
    echo "     http://$HOST:$SVC_PORT/v1/pipeline/run"
    echo "═══════════════════════════════════════════════════════"
else
    echo "[health] ✗ health check failed"
    echo "         Debug: ssh $RUSER@$HOST 'journalctl -u sovereign-cloud -n 40'"
    exit 1
fi
