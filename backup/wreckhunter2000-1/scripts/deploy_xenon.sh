#!/usr/bin/env bash
# Deploy sovereign-cloud + model-team-tool to the XENON (GTX 1070, 8GB).
#
# XENON role in the CESARops network:
#   sovereign-cloud :8765  — pipeline node, forwards scout to i7 TPU
#   model-team-tool :8766  — reasoning+coding daemon (backed by KoboldCpp)
#
# KoboldCpp is expected to be already installed and running on XENON at:
#   Port 5001 — reasoning model (e.g. deepseek-r1-7b or qwen2.5-7b)
#   Port 5002 — coding model   (e.g. qwen2.5-coder-7b)
#
# Usage:
#   bash scripts/deploy_xenon.sh              # full deploy (sync + build + restart)
#   bash scripts/deploy_xenon.sh --restart    # restart services only
#   bash scripts/deploy_xenon.sh --build-only # build + install, skip sync

set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"

if [[ -f "$REPO/.env" ]]; then
    set -a; source "$REPO/.env"; set +a
fi

HOST="${XENON_HOST:-10.0.0.129}"
RUSER="${XENON_USER:-cesarops1}"
KEY="${XENON_KEY:-}"
WORK="${XENON_WORK:-/home/$RUSER/wreckhunter2000-1}"
SSHPORT="${XENON_SSH_PORT:-22}"

# XENON LLM backend — KoboldCpp already installed
REASONING_URL="${REASONING_BASE_URL:-http://localhost:5001/v1}"
CODING_URL="${CODING_BASE_URL:-http://localhost:5002/v1}"
LLM_URL="${LLM_BASE_URL:-http://localhost:5001/v1}"

# i7 TPU server (Coral TPU + P1000) — XENON offloads scout pass here
TPU_URL="${TPU_SERVER_URL:-http://100.85.138.4:8765}"

SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -p $SSHPORT"
[[ -n "$KEY" ]] && SSH_OPTS="$SSH_OPTS -i $KEY"
ssh_run() { ssh $SSH_OPTS "$RUSER@$HOST" "$@"; }

MODE="full"
case "${1:-}" in
    --restart)    MODE="restart"    ;;
    --build-only) MODE="build"      ;;
esac

echo "═══════════════════════════════════════════════════════"
echo "  Deploy to XENON  $RUSER@$HOST"
echo "  mode: $MODE   dir: $WORK"
echo "  KoboldCpp reasoning: $REASONING_URL"
echo "  KoboldCpp coding:    $CODING_URL"
echo "  TPU server (i7):     $TPU_URL"
echo "═══════════════════════════════════════════════════════"

# ── Connectivity ──────────────────────────────────────────────────────────────
echo ""
echo "[check] SSH..."
if ! ssh $SSH_OPTS "$RUSER@$HOST" true 2>/dev/null; then
    echo "[error] Cannot reach $HOST — check VPN/LAN and SSH keys"
    exit 1
fi
echo "[check] OK"

if [[ "$MODE" == "restart" ]]; then
    ssh_run "sudo systemctl restart sovereign-cloud model-team-tool 2>/dev/null; sudo systemctl status sovereign-cloud model-team-tool --no-pager -l"
    exit 0
fi

# ── Sync source ───────────────────────────────────────────────────────────────
if [[ "$MODE" == "full" ]]; then
    echo ""
    echo "[sync] Source → $WORK..."
    REMOTE_HAS_REPO=$(ssh_run "[ -d '$WORK/.git' ] && echo yes || echo no")

    if [[ "$REMOTE_HAS_REPO" == "yes" ]]; then
        echo "[sync] git pull..."
        ssh_run "cd '$WORK' && git pull --ff-only"
    else
        echo "[sync] rsync..."
        ssh_run "mkdir -p '$WORK'"

        for dir in sovereign-cloud nauticuvs model-team-tool cesarops-slicer; do
            rsync -az --delete \
                --exclude='target/' \
                -e "ssh $SSH_OPTS" \
                "$REPO/$dir/" "$RUSER@$HOST:$WORK/$dir/"
        done

        [[ -f "$REPO/Cargo.lock" ]] && \
            rsync -az -e "ssh $SSH_OPTS" "$REPO/Cargo.lock" "$RUSER@$HOST:$WORK/Cargo.lock"

        # Workspace Cargo.toml covering all deployed crates
        ssh_run "cat > '$WORK/Cargo.toml'" << 'TOML'
[workspace]
members = ["sovereign-cloud", "nauticuvs", "cesarops-slicer"]
resolver = "2"
TOML
        echo "[sync] Done."
    fi
fi

# ── Ensure Rust ───────────────────────────────────────────────────────────────
echo ""
echo "[rust] Checking remote Rust..."
if ! ssh_run "source \$HOME/.cargo/env 2>/dev/null; command -v cargo > /dev/null 2>&1"; then
    echo "[rust] Installing rustup..."
    ssh_run "curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path"
fi
echo "[rust] OK"

# ── Build deps ────────────────────────────────────────────────────────────────
echo ""
echo "[deps] Build tools + NVIDIA headers..."
ssh_run "
    if ! command -v gcc > /dev/null 2>&1; then
        sudo apt-get update -qq
        sudo apt-get install -y --no-install-recommends \
            build-essential pkg-config libssl-dev
    fi
    # nvml-wrapper needs the nvidia management library headers
    if ! dpkg -l libnvidia-ml-dev 2>/dev/null | grep -q '^ii'; then
        sudo apt-get install -y --no-install-recommends libnvidia-ml-dev 2>/dev/null || true
    fi
    echo 'deps ok'
"

# ── Build sovereign-cloud ─────────────────────────────────────────────────────
echo ""
echo "[build] sovereign-cloud (release)..."
ssh_run "
    source \$HOME/.cargo/env
    cd '$WORK'
    cargo build --release -p sovereign-cloud
"
echo "[build] sovereign-cloud done."

# ── Build model-team-tool ─────────────────────────────────────────────────────
echo ""
echo "[build] model-team-tool (release)..."
ssh_run "
    source \$HOME/.cargo/env
    cd '$WORK/model-team-tool'
    cargo build --release
"
echo "[build] model-team-tool done."

# ── Build research_ingestion_specialist ───────────────────────────────────────
echo ""
echo "[build] research_ingestion_specialist (release)..."
echo "        (First run: extra time for pdf-extract deps)"
ssh_run "
    source \$HOME/.cargo/env
    cd '$WORK'
    cargo build --release -p cesarops-slicer --bin research_ingestion_specialist
"
echo "[build] research_ingestion_specialist done."

# ── Install binaries ──────────────────────────────────────────────────────────
echo ""
echo "[install] /opt/cesarops/..."
ssh_run "
    sudo mkdir -p /opt/cesarops
    sudo cp '$WORK/target/release/sovereign-cloud'              /opt/cesarops/sovereign-cloud
    sudo cp '$WORK/model-team-tool/target/release/model-team-tool' /opt/cesarops/model-team-tool
    sudo cp '$WORK/target/release/research_ingestion_specialist' /opt/cesarops/research_ingestion_specialist
    sudo chmod +x /opt/cesarops/sovereign-cloud /opt/cesarops/model-team-tool /opt/cesarops/research_ingestion_specialist
    echo 'installed.'
"

# ── Systemd: sovereign-cloud ──────────────────────────────────────────────────
echo ""
echo "[service] sovereign-cloud.service..."
ssh_run "sudo tee /etc/systemd/system/sovereign-cloud.service > /dev/null" << EOF
[Unit]
Description=CESARops sovereign-cloud node API (XENON)
After=network.target

[Service]
ExecStart=/opt/cesarops/sovereign-cloud
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
Environment="LLM_BASE_URL=$LLM_URL"
Environment="TPU_SERVER_URL=$TPU_URL"

[Install]
WantedBy=multi-user.target
EOF

# ── Systemd: model-team-tool ──────────────────────────────────────────────────
echo "[service] model-team-tool.service..."
ssh_run "sudo tee /etc/systemd/system/model-team-tool.service > /dev/null" << EOF
[Unit]
Description=CESARops model-team-tool coding agent daemon (XENON)
After=network.target

[Service]
ExecStart=/opt/cesarops/model-team-tool --mode daemon --port 8766
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=5
Environment="RUST_LOG=info"
Environment="REASONING_BASE_URL=$REASONING_URL"
Environment="CODING_BASE_URL=$CODING_URL"

[Install]
WantedBy=multi-user.target
EOF

# ── Systemd: research_ingestion_specialist ────────────────────────────────────
echo "[service] research-ingestion.service..."
ssh_run "sudo tee /etc/systemd/system/research-ingestion.service > /dev/null" << EOF
[Unit]
Description=CESARops Research Ingestion Specialist — arXiv/S2 scraper + LLM synthesis
After=network.target

[Service]
ExecStart=/opt/cesarops/research_ingestion_specialist --daemon --interval-minutes 120
WorkingDirectory=/opt/cesarops
Restart=on-failure
RestartSec=30
Environment="RUST_LOG=info"
Environment="REASONING_BASE_URL=$REASONING_URL"

[Install]
WantedBy=multi-user.target
EOF

# ── Enable + start ────────────────────────────────────────────────────────────
ssh_run "
    sudo systemctl daemon-reload
    sudo systemctl enable sovereign-cloud model-team-tool research-ingestion
    sudo systemctl restart sovereign-cloud model-team-tool research-ingestion
    sleep 3
    sudo systemctl status sovereign-cloud model-team-tool research-ingestion --no-pager -l
"

# ── Health checks ─────────────────────────────────────────────────────────────
echo ""
echo "[health]"
SC=$(ssh_run "curl -sf http://localhost:8765/health && echo ok || echo fail")
MT=$(ssh_run "curl -sf http://localhost:8766/health && echo ok || echo fail")
echo "  sovereign-cloud  :8765  $SC"
echo "  model-team-tool  :8766  $MT"

if [[ "$SC" == "ok" && "$MT" == "ok" ]]; then
    echo ""
    echo "═══════════════════════════════════════════════════════"
    echo "  ✓  XENON is live"
    echo ""
    echo "     $HOST:8765/v1/node/status"
    echo "     $HOST:8766/health"
    echo ""
    echo "  Next: start KoboldCpp on XENON"
    echo "    koboldcpp --port 5001 --usecublas --model <reasoning.gguf>"
    echo "    koboldcpp --port 5002 --usecublas --model <coding.gguf>"
    echo "═══════════════════════════════════════════════════════"
else
    echo "[warn] One or more services not responding — check:"
    echo "       ssh $RUSER@$HOST 'journalctl -u sovereign-cloud -n 20'"
    echo "       ssh $RUSER@$HOST 'journalctl -u model-team-tool -n 20'"
fi
