#!/usr/bin/env bash
# CESAROPS pipeline runner — the bridge between n8n and the Rust detection
# binaries. n8n Execute Command nodes call this; it validates the pipeline
# name, resolves the built binary, and shells the Rust tool.
#
# Usage:
#   pipeline_runner.sh catalog
#       Emit the combined --describe catalog of all pipelines (JSON).
#   pipeline_runner.sh describe <satellite|aeromag|bag>
#       Emit one pipeline's tool catalog (JSON).
#   pipeline_runner.sh run satellite  --spec <file> [--knobs <json>] [--stages ...] [--dry-run]
#   pipeline_runner.sh run aeromag    --grid <bin> --meta <json> --levels <json> [--wells <csv>] [--pixel-size N]
#   pipeline_runner.sh run bag        <path.bag> [--knobs <json>] [--stages a,c,g]
#
# Env:
#   CARGO_TARGET_DIR  (default /data/cargo-target) — where the binaries live
#   REPO              (default autodetected) — repo root
set -euo pipefail

REPO="${REPO:-$(cd "$(dirname "$0")/.." && pwd)}"
TARGET_DIR="${CARGO_TARGET_DIR:-/data/cargo-target}"
PROFILE="${PROFILE:-debug}"
BIN_DIR="$TARGET_DIR/$PROFILE"

SAT_BIN="$BIN_DIR/sat-run"
MAG_BIN="$BIN_DIR/cesarops-aeromagnetic-worker"
BAG_BIN="$BIN_DIR/cesarops-bag-scan"

err() { echo "{\"error\": \"$*\"}" >&2; exit 1; }

resolve_bin() {
  case "$1" in
    satellite) echo "$SAT_BIN" ;;
    aeromag|aeromagnetic|mag) echo "$MAG_BIN" ;;
    bag) echo "$BAG_BIN" ;;
    *) err "unknown pipeline '$1' (expected satellite|aeromag|bag)" ;;
  esac
}

describe_one() {
  local pipe="$1" bin
  bin="$(resolve_bin "$pipe")"
  [[ -x "$bin" ]] || err "binary not built: $bin (run cargo build)"
  case "$pipe" in
    satellite)            "$bin" --describe ;;
    aeromag|aeromagnetic|mag) "$bin" describe ;;
    bag)                  "$bin" --describe ;;
  esac
}

cmd="${1:-}"; shift || true
case "$cmd" in
  catalog)
    # Combine all three describe outputs into one JSON document.
    printf '{\n  "generated_at": "%s",\n  "pipelines": [\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)"
    first=1
    for p in satellite aeromag bag; do
      if [[ $first -eq 0 ]]; then printf ',\n'; fi
      first=0
      describe_one "$p"
    done
    printf '\n  ]\n}\n'
    ;;
  describe)
    [[ $# -ge 1 ]] || err "describe needs a pipeline name"
    describe_one "$1"
    ;;
  run)
    [[ $# -ge 1 ]] || err "run needs a pipeline name"
    pipe="$1"; shift
    bin="$(resolve_bin "$pipe")"
    [[ -x "$bin" ]] || err "binary not built: $bin (run cargo build)"
    case "$pipe" in
      aeromag|aeromagnetic|mag)
        # mag uses a `detect` subcommand
        exec "$bin" detect "$@"
        ;;
      *)
        exec "$bin" "$@"
        ;;
    esac
    ;;
  *)
    err "usage: pipeline_runner.sh {catalog|describe <pipe>|run <pipe> ...}"
    ;;
esac
