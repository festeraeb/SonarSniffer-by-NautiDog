#!/usr/bin/env python3
import asyncio
import json
import websockets

async def mock_n8n_agent(websocket, path):
    print(f"[{path}] Connected to local Home Forge...")
    try:
        async for message in websocket:
            print(f"Received Telemetry Payload:\n{message}")
            
            # Simulate processing time
            print("Diagnosing issue...")
            await asyncio.sleep(2)
            
            fix_payload = {
                "action": "run_powershell",
                "script": "Write-Host 'Hello from the Autonomous Cloud Agent! Fix successfully executed locally.'; Start-Sleep -Seconds 2"
            }
            
            print("Streaming Fix Payload back to client...")
            await websocket.send(json.dumps(fix_payload))
            
    except websockets.exceptions.ConnectionClosed:
        print("Connection closed.")

start_server = websockets.serve(mock_n8n_agent, "localhost", 8765)

print("Starting Mock n8n Cloud Agent on ws://localhost:8765...")
asyncio.get_event_loop().run_until_complete(start_server)
asyncio.get_event_loop().run_forever()
