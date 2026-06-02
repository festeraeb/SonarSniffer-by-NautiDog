#!/usr/bin/env bash
# T440 — Lock-3 jitter analyst (Rust). Pure-Rust tract CPU primary inference with
# heterogeneous accelerator cross-validation (Movidius NCS2 on this node; Coral
# Edge TPU on the ML350e). Binds :8180 for the fleet detection pool.
#
# Replaces the legacy Python jitter_movidius.py path, which silently fell back to
# CPU because the installed OpenVINO (2026.x) dropped the MYRIAD plugin.
set -euo pipefail

export PATH="${HOME}/.cargo/bin:${PATH}"

REPO="${REPO:-/codebase/repos/wreckhunter2000-1}"
CRATE_DIR="$REPO/cesarops-detection/jitter-rs"
PORT="${JITTER_PORT:-8180}"
# ML350e (cesarops2) — Coral jitter validator (HTTP sidecar; Lock-3 primary stays Rust).
ML350E_LAN="${ML350E_LAN:-10.0.0.201}"
INSTALL_BIN="${JITTER_RS_INSTALL_PATH:-/opt/cesarops/bin/jitter-rs}"

log() { echo "[t440-jitter-rs] $*"; }

# Remote Coral vote (optional; jitter-rs probes at startup and skips if down).
if [[ -z "${JITTER_REMOTE_VALIDATORS:-}" ]]; then
  export JITTER_REMOTE_VALIDATORS="coral_edgetpu=http://${ML350E_LAN}:8190"
fi
log "remote validators: $JITTER_REMOTE_VALIDATORS"

# Feature flags: enable the Movidius validator when the NCS2 is attached *and*
# an OpenVINO 2022.3 runtime is available (typically inside a 22.04 container).
FEATURES=()
if lsusb 2>/dev/null | grep -q '03e7:'; then
  log "Movidius NCS2 (03e7) detected on USB"
  # Only request the movidius feature when the legacy runtime is reachable.
  if [[ -n "${OPENVINO_2022_RUNTIME:-}" && -d "${OPENVINO_2022_RUNTIME}" ]]; then
    FEATURES+=("movidius")
  else
    log "NOTE: OPENVINO_2022_RUNTIME not set — Movidius runs in detect-only validator mode"
    FEATURES+=("movidius")  # sysfs detection still records the device as a validator
  fi
fi

FEATURE_ARG=""
if [[ ${#FEATURES[@]} -gt 0 ]]; then
  FEATURE_ARG="--features $(IFS=,; echo "${FEATURES[*]}")"
fi

resolve_target_bin() {
  local target_dir
  target_dir="$( cd "$CRATE_DIR" && cargo metadata --no-deps --format-version 1 2>/dev/null \
    | python3 -c 'import sys,json; print(json.load(sys.stdin)["target_directory"])')"
  echo "${target_dir}/release/jitter-rs"
}

if [[ -n "${JITTER_RS_BIN:-}" && -x "${JITTER_RS_BIN}" ]]; then
  BIN="$JITTER_RS_BIN"
  log "using JITTER_RS_BIN=$BIN"
elif [[ -x "$INSTALL_BIN" && "${JITTER_RS_SKIP_BUILD:-}" == "1" ]]; then
  BIN="$INSTALL_BIN"
  log "using installed binary (JITTER_RS_SKIP_BUILD=1) $BIN"
else
  log "building jitter-rs (release) ${FEATURE_ARG:-[default features]}"
  ( cd "$CRATE_DIR" && cargo build --release ${FEATURE_ARG} )
  BIN="$(resolve_target_bin)"
  if [[ "${JITTER_RS_DO_INSTALL:-}" == "1" && -x "$BIN" ]]; then
    mkdir -p "$(dirname "$INSTALL_BIN")"
    install -m 0755 "$BIN" "$INSTALL_BIN"
    BIN="$INSTALL_BIN"
    log "installed to $BIN (set JITTER_RS_SKIP_BUILD=1 on next Nomad restart)"
  fi
fi

# -x only matches the binary name (not this script path, which contains "jitter-rs").
pkill -x jitter-rs 2>/dev/null || true
pkill -f "jitter_movidius.py" 2>/dev/null || true
fuser -k "${PORT}/tcp" 2>/dev/null || true
sleep 1

export JITTER_PORT="$PORT"
export JITTER_MODEL="${JITTER_MODEL:-}"   # optional ONNX model path
export RUST_LOG="${RUST_LOG:-info}"

log "starting jitter-rs on :$PORT (foreground for Nomad)"
exec "$BIN"
