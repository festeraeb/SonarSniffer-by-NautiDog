#!/bin/bash
# Test the MCP server on T440 with proper JSON
export LLM_URL=http://100.102.158.111:5555/v1
export NAUTIVECS_DB=/home/cesarops/cesarops-mcp-steered/data/nautivecs_store.json
MCP=/home/cesarops/cesarops-mcp-steered/target/release/cesarops-mcp

echo '{"jsonrpc":"2.0","id":1,"method":"tools/list","params":{}}' | $MCP > /tmp/mcp_result.txt 2>/tmp/mcp_log.txt

echo "=== MCP Response ==="
cat /tmp/mcp_result.txt | python3 -m json.tool 2>/dev/null || cat /tmp/mcp_result.txt
echo ""
echo "=== MCP Log ==="
tail -5 /tmp/mcp_log.txt
