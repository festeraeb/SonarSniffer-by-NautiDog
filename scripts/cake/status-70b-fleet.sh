#!/usr/bin/env bash
# Quick fleet readiness check (T440 + cesarops2 ports + :8081).
set -uo pipefail

CESAROPS2_HOST="${CESAROPS2_HOST:-10.0.0.201}"
SKIP_P106="${SKIP_P106:-1}"
CAKE_DUAL_P100="${CAKE_DUAL_P100:-1}"
check() {
  local label="$1" host="$2" port="$3"
  if timeout 2 bash -c "echo >/dev/tcp/${host}/${port}" 2>/dev/null; then
    echo "  OK   $label :$port"
  else
    echo "  DOWN $label :$port"
  fi
}

echo "=== Cake 70B fleet status (SKIP_P106=${SKIP_P106}) ==="
[[ "$SKIP_P106" == "1" ]] || check "c2-p106" "$CESAROPS2_HOST" 10128
check "c2-rtx"  "$CESAROPS2_HOST" 10129
check "c2-1070" "$CESAROPS2_HOST" 10130
[[ "$CAKE_DUAL_P100" == "1" ]] && check "t440-p100-0" 127.0.0.1 10133
check "t440-p100-1" 127.0.0.1 10131
check "t440-ram"    127.0.0.1 10132
if curl -sf --max-time 3 http://127.0.0.1:8081/v1/models >/dev/null 2>&1; then
  echo "  OK   master API :8081"
else
  echo "  DOWN master API :8081"
fi
echo ""
pgrep -af 'cake (master|worker)' 2>/dev/null | grep -v status || echo "(no cake processes)"
echo ""
echo "T440 log: tail -3 ~/.cache/cesarops/cake_fleet.log"
tail -3 "${HOME}/.cache/cesarops/cake_fleet.log" 2>/dev/null || true
