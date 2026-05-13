#!/bin/bash
# Test the full SCM pipeline: T440 (controller) → cesarops2 (LLM)
# RSU: assign_weather_weight precision task
set -e

export LLM_URL=http://100.102.158.111:5555/v1
export NAUTIVECS_DB=/home/cesarops/cesarops-mcp-steered/data/nautivecs_store.json
export LLM_MODEL=qwen2.5-coder-3b-instruct
MCP=/home/cesarops/cesarops-mcp-steered/target/release/cesarops-mcp

echo "=== Weather RSU Test — Full Pipeline ==="
echo "Controller: T440 (dual Xeon)"
echo "LLM: cesarops2 (1070, Qwen2.5-Coder-3B)"
echo ""

# Test 1: Health check
echo "--- Health Check ---"
echo '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"health","arguments":{}}}' | $MCP 2>/dev/null
echo ""

# Test 2: Steered query — the weather weight RSU
echo "--- Weather RSU (steered_query) ---"
echo '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"steered_query","arguments":{"query":"Write a Rust function called assign_weather_weight that takes a WeatherWindow enum and returns f32. PostStorm day 1 returns 3.0, Calm returns 1.0, all others return 0.5. Reference the scan-strategy.md rules.","role":"sensor"}}}' | $MCP 2>/dev/null
echo ""

# Test 3: Tune parameters
echo "--- Tune Parameters ---"
echo '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"tune_parameters","arguments":{"task":"Set glint detection threshold for calm water tiles in Lake Erie","constraints":"calm weather, shallow depth 10-20m, Sentinel-2 optical"}}}' | $MCP 2>/dev/null
echo ""

echo "=== Test Complete ==="
