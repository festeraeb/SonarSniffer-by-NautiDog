"""
End-to-end test for the CESAROPS Thought Engine.
Sends a task, waits for completion, and prints results.
"""

import asyncio
import json
import sys
import time
import httpx

THOUGHT_ENGINE_URL = "http://localhost:5556"

async def send_and_wait_task(client: httpx.AsyncClient, query: str) -> dict:
    print(f"\n[TEST] Sending task: '{query}'")
    
    # 1. Send Task
    resp = await client.post(
        "/tasks",
        json={"query": query, "task_id": "test_001"}
    )
    resp.raise_for_status()
    task_data = resp.json()
    print(f"[TEST] Task created: {task_data['task_id']}, Status: {task_data['status']}")
    
    # 2. Poll for Completion
    max_retries = 60  # 5 minutes max
    for i in range(max_retries):
        await asyncio.sleep(5)
        get_resp = await client.get(f"/tasks/test_001")
        get_resp.raise_for_status()
        task_data = get_resp.json()
        
        print(f"[TEST] Poll {i+1}: Status={task_data['status']}, Steps={len(task_data['steps'])}")
        
        if task_data['status'] in ['completed', 'completed_with_warnings', 'failed']:
            break
    
    return task_data

async def main():
    async with httpx.AsyncClient(base_url=THOUGHT_ENGINE_URL, timeout=300.0) as client:
        # Health Check
        try:
            health = await client.get("/health")
            print(f"[TEST] Health: {health.json()}")
        except Exception as e:
            print(f"[TEST] Health check failed: {e}")
            print("Is the Thought Engine running? Start it with: python thought_engine.py")
            sys.exit(1)
        
        # Test Query
        query = "Build a Rust function that calculates tidal coefficients for Lake Michigan using harmonic constants."
        
        result = await send_and_wait_task(client, query)
        
        print("\n=== FINAL RESULT ===")
        print(f"Status: {result['status']}")
        if result.get('error'):
            print(f"Error: {result['error']}")
        if result.get('result'):
            print(f"Result (first 500 chars):\n{result['result'][:500]}...")
        
        print("\n=== STEPS ===")
        for step in result.get('steps', []):
            print(f"- {step.get('step')}: {json.dumps(step).get('output', '')[:100]}")

if __name__ == "__main__":
    asyncio.run(main())
