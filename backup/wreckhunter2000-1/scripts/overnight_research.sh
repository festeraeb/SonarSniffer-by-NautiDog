#!/bin/bash
# CESAROPS Overnight Research Loop
# ================================
# Runs the research ingestion specialist in daemon mode using Qwen3.6-35B
# on the dual P100s. Designed to run while you sleep.
#
# Usage:
#   bash scripts/overnight_research.sh          # Start daemon
#   bash scripts/overnight_research.sh --stop   # Stop daemon
#   bash scripts/overnight_research.sh --status # Check status
#
# Prerequisites:
#   - KoboldCPP running with Qwen3.6-35B on port 5001
#   - research_ingestion_specialist binary built
#   - Internet access for arXiv/Semantic Scholar queries

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$SCRIPT_DIR")"
PID_FILE="/tmp/cesarops-research.pid"
LOG_FILE="$REPO_DIR/research_log/overnight_$(date +%Y-%m-%d).log"

# Ensure log directory exists
mkdir -p "$REPO_DIR/research_log"

# LLM endpoint — Qwen3.6-35B on dual P100s via Cloudflare tunnel
export REASONING_BASE_URL="https://llm.cesarops.org/v1"
export LLM_BASE_URL="https://llm.cesarops.org/v1"

case "${1:-}" in
    --stop)
        if [ -f "$PID_FILE" ]; then
            PID=$(cat "$PID_FILE")
            if kill -0 "$PID" 2>/dev/null; then
                kill "$PID"
                echo "Stopped research daemon (PID $PID)"
            else
                echo "Process $PID not running"
            fi
            rm -f "$PID_FILE"
        else
            echo "No PID file found"
        fi
        exit 0
        ;;
    --status)
        if [ -f "$PID_FILE" ]; then
            PID=$(cat "$PID_FILE")
            if kill -0 "$PID" 2>/dev/null; then
                echo "Research daemon running (PID $PID)"
                echo "Log: $LOG_FILE"
                echo "Last 5 lines:"
                tail -5 "$LOG_FILE" 2>/dev/null || echo "(no log yet)"
            else
                echo "PID file exists but process $PID is dead"
                rm -f "$PID_FILE"
            fi
        else
            echo "Research daemon not running"
        fi
        exit 0
        ;;
esac

# Check if KoboldCPP is reachable
echo "Checking LLM endpoint..."
if ! curl -s --max-time 5 "https://llm.cesarops.org/v1/models" > /dev/null 2>&1; then
    echo "ERROR: KoboldCPP not reachable at https://llm.cesarops.org"
    echo "Start it first: sudo systemctl start koboldcpp"
    exit 1
fi
echo "LLM endpoint OK"

# Check if binary exists
BINARY="$REPO_DIR/target/release/cesarops-slicer"
if [ ! -f "$BINARY" ]; then
    echo "Building research_ingestion_specialist..."
    cd "$REPO_DIR"
    cargo build --release --package cesarops-slicer 2>&1 | tail -5
fi

# Start daemon
echo "Starting overnight research daemon..."
echo "  LLM: $REASONING_BASE_URL"
echo "  DB:  $REPO_DIR/research.db"
echo "  Log: $LOG_FILE"
echo ""

nohup "$BINARY" \
    --daemon \
    --interval-minutes 30 \
    --max-results 10 \
    --db-path "$REPO_DIR/research.db" \
    --llm-url "$REASONING_BASE_URL" \
    >> "$LOG_FILE" 2>&1 &

echo $! > "$PID_FILE"
echo "Research daemon started (PID $(cat $PID_FILE))"
echo "Check status: bash scripts/overnight_research.sh --status"
echo "Stop:         bash scripts/overnight_research.sh --stop"
