#!/usr/bin/env bash
# Read-only checklist — run after satellite pipeline queue is clear.
set -euo pipefail

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
FORGE_URL="${FORGE_URL:-http://127.0.0.1:9100}"
RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

ok() { echo -e "${GREEN}[ok]${NC} $*"; }
warn() { echo -e "${RED}[pending]${NC} $*"; }

echo "=== CESAROPS post-pipeline checklist ==="

if curl -sf --max-time 3 "${FORGE_URL}/webhook/missions" 2>/dev/null | grep -q '"status":"running"'; then
  warn "Forge mission still running — wait before restart / fleet enable"
else
  ok "No running Forge missions (or Forge unreachable)"
fi

if pgrep -f koboldcpp >/dev/null 2>&1; then
  warn "koboldcpp still running — should stay masked"
else
  ok "koboldcpp not running"
fi

CAKE_BIN="/opt/cesarops/cake/bin/cake"
[[ -x "$CAKE_BIN" ]] || CAKE_BIN="${HOME}/.cargo/bin/cake"
if [[ -x "$CAKE_BIN" ]]; then
  ok "Cake binary: $CAKE_BIN"
else
  warn "Cake not installed — bash scripts/install_cake_fleet.sh"
fi

if [[ -f /etc/cesarops/fleet-mode.enabled ]]; then
  ok "Fleet mode flag enabled"
else
  warn "Fleet disabled — sudo touch /etc/cesarops/fleet-mode.enabled when ready"
fi

if [[ -f "${REPO}/cesarops-forge-v2/target/release/cesarops-forge-v2" ]]; then
  ok "Forge v2 release binary built"
else
  warn "Build Forge: cd cesarops-forge-v2 && cargo build --release"
fi

echo ""
echo "Suggested sequence:"
echo "  1. sudo systemctl restart cesarops-forge-v2"
echo "  2. bash scripts/p100_cycle.sh restore"
echo "  3. bash scripts/install_cake_fleet.sh   # on T440 + cesarops2"
echo "  4. sudo systemctl enable --now cesarops-activity-watch.timer"
echo "  5. Import missions/n8n_predictive_async_moe.json when PAMP shadow wins"
echo "  6. python scripts/deploy_web.py && python scripts/deploy_web.py --ionos"
