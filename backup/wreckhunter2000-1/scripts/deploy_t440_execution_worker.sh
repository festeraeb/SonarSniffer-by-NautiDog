#!/usr/bin/env bash
# Deploy split-agent coder to T440 (10.0.0.61) as ExecutionWorker
# Windows-compatible version (uses git instead of rsync)

set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -f "$REPO/.env" ]]; then
    set -a; source "$REPO/.env"; set +a
fi

HOST="${T440_TAILSCALE:-${T440_HOST:-10.0.0.61}}"
RUSER="${T440_USER:-cesarops}"
KEY="${T440_KEY:-}"
WORK="${T440_WORK:-/home/cesarops/wreckhunter2000-1}"
CODING_URL="${CODING_BASE_URL:-http://localhost:5001/v1}"
SSHPORT="${T440_SSH_PORT:-22}"
DAEMON_PORT=8766
DEPLOY_BIN="/opt/cesarops/split-agent"

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
    --restart)     MODE="restart" ;;
    --build-only)  MODE="build"   ;;
esac

echo "═══════════════════════════════════════════════════════"
echo "  split-agent → ExecutionWorker (T440) at $RUSER@$HOST:$DAEMON_PORT"
echo "  mode: $MODE   remote dir: $WORK"
echo "═══════════════════════════════════════════════════════"

echo ""
echo "[check] SSH connectivity..."
if ! ssh $SSH_OPTS "$RUSER@$HOST" true 2>/dev/null; then
    echo "[error] Cannot reach $HOST"
    exit 1
fi
echo "[check] OK"

if [[ "$MODE" == "restart" ]]; then
    ssh_run "sudo systemctl restart split-agent-daemon && sudo systemctl status split-agent-daemon --no-pager -l"
    exit 0
fi

if [[ "$MODE" == "full" || "$MODE" == "sync" ]]; then
    echo ""
    echo "[sync] Checking for repo on remote..."

    REMOTE_HAS_REPO=$(ssh_run "[ -d '$WORK/.git' ] && echo yes || echo no")

    if [[ "$REMOTE_HAS_REPO" == "yes" ]]; then
        echo "[sync] Repo exists — git pull..."
        ssh_run "cd '$WORK' && git pull --ff-only"
    else
        echo "[sync] No repo — setting up..."
        ssh_run "mkdir -p '$WORK' && cd '$WORK' && git init && git remote add origin https://github.com/wreckhunter2000/wreckhunter2000-1.git && git pull origin main"
    fi

    [[ "$MODE" == "sync" ]] && exit 0
fi

echo ""
echo "[rust] Checking Rust on remote..."
if ! ssh_run "source \$HOME/.cargo/env 2>/dev/null; command -v cargo > /dev/null 2>&1"; then
    echo "[rust] Installing via rustup..."
    ssh_run "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path"
fi
echo "[rust] OK"

echo ""
echo "[deps] Checking build tools..."
ssh_run "
    if ! command -v gcc > /dev/null 2>&1; then
        echo 'Installing build-essential...'
        sudo apt-get update -qq
        sudo apt-get install -y --no-install-recommends build-essential pkg-config libssl-dev
    else
        echo 'gcc present'
    fi
"

echo ""
echo "[build] Building split-agent --release on remote..."
ssh_run "
    source \$HOME/.cargo/env
    cd '$WORK'
    cargo build --release -p model-team-tool
"
echo "[build] Done."

echo ""
echo "[install] Copying binary to $DEPLOY_BIN..."
ssh_run "
    sudo mkdir -p /opt/cesarops
    sudo cp '$WORK/target/release/model-team-tool' $DEPLOY_BIN
    sudo chmod +x $DEPLOY_BIN
    echo 'Installed.'
"

echo ""
echo "[service] Installing/updating split-agent-daemon.service..."
ssh_run "sudo tee /etc/systemd/system/split-agent-daemon.service > /dev/null" << EOF
[Unit]
Description=CESARops split-agent ExecutionWorker (daemon mode)
After=network.target

[Service]
Type=simple
ExecStart=$DEPLOY_BIN --mode daemon --port $DAEMON_PORT
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
Environment="CODING_BASE_URL=$CODING_URL"

[Install]
WantedBy=multi-user.target
EOF

ssh_run "
    sudo systemctl daemon-reload
    sudo systemctl enable split-agent-daemon
    sudo systemctl restart split-agent-daemon
    sleep 2
    sudo systemctl status split-agent-daemon --no-pager -l
"

echo ""
echo "[health] Checking daemon..."
sleep 2
if ssh_run "curl -sf http://localhost:$DAEMON_PORT/health 2>&1"; then
    echo ""
    echo "═══════════════════════════════════════════════════════"
    echo "  ✓ split-agent ExecutionWorker is live on T440"
    echo ""
    echo "     http://$HOST:$DAEMON_PORT/health"
    echo "     http://$HOST:$DAEMON_PORT/ready"
    echo ""
    echo "  Dispatch code jobs from i7:"
    echo "     POST http://10.0.0.61:$DAEMON_PORT/brief -d @brief.json"
    echo "═══════════════════════════════════════════════════════"
else
    echo "[health] ✗ health check failed"
    echo "         Debug: ssh $RUSER@$HOST 'journalctl -u split-agent-daemon -n 40'"
    exit 1
fi
