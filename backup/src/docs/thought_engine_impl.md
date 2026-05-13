

=== FILE: scripts/setup_thought_engine.sh ===
```bash
#!/usr/bin/env bash
# setup_thought_engine.sh
# Runs on cesarops2 (100.102.158.111)
# Downloads Qwen3-8B, replaces the 3B model, sets up the Thought Engine daemon.

set -euo pipefail

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CESAROPS2_HOME="/home/cesarops" # Adjust if needed
KOBOLD_CPP_BIN="${CESAROPS2_HOME}/benchmark/koboldcpp"
MODELS_DIR="${CESAROPS2_HOME}/benchmark/models"
VENV_DIR="${CESAROPS2_HOME}/thought_engine_venv"
MODEL_NAME="Qwen3-8B-Q4_K_M.gguf"
MODEL_URL="https://huggingface.co/bartowski/Qwen3-8B-GGUF/resolve/main/Qwen3-8B-Q4_K_M.gguf"
KOBOLD_PORT=5555
THOUGHT_ENGINE_PORT=5556

echo "=== CESAROPS Thought Engine Setup ==="
echo "Target: cesarops2 (${CESAROPS2_HOME})"
echo "Model: ${MODEL_NAME}"

# 1. Create Python venv and install deps
echo "[1/5] Setting up Python environment..."
if [ ! -d "${VENV_DIR}" ]; then
    python3 -m venv "${VENV_DIR}"
fi
source "${VENV_DIR}/bin/activate"
pip install --upgrade pip
pip install huggingface-hub httpx fastapi uvicorn pydantic

# 2. Download Model
echo "[2/5] Downloading Qwen3-8B-Q4_K_M..."
MODEL_PATH="${MODELS_DIR}/${MODEL_NAME}"
if [ ! -f "${MODEL_PATH}" ]; then
    echo "  Downloading via huggingface-cli..."
    huggingface-cli download bartowski/Qwen3-8B-GGUF \
        --filename Qwen3-8B-Q4_K_M.gguf \
        --local-dir "${MODELS_DIR}"
else
    echo "  Model already exists at ${MODEL_PATH}. Skipping download."
fi

# 3. Kill existing KoboldCPP (3B model)
echo "[3/5] Stopping existing KoboldCPP processes..."
pkill -f "koboldcpp.*3B" || true
sleep 2

# 4. Start KoboldCPP with Qwen3-8B on port 5555
echo "[4/5] Starting KoboldCPP with Qwen3-8B on port ${KOBOLD_PORT}..."
nohup "${KOBOLD_CPP_BIN}" \
    --model "${MODEL_PATH}" \
    --port "${KOBOLD_PORT}" \
    --gpulayers 40 \
    --contextsize 8192 \
    --threads 8 \
    --log-disable \
    > /var/log/kobold-qwen3-8b.log 2>&1 &
KOBOLD_PID=$!
echo "  KoboldCPP started (PID ${KOBOLD_PID})"

# Wait for Kobold to be ready
echo "  Waiting for KoboldCPP to initialize..."
for i in {1..30}; do
    if curl -s http://localhost:${KOBOLD_PORT}/health > /dev/null 2>&1; then
        echo "  KoboldCPP is ready."
        break
    fi
    sleep 1
done

# 5. Install systemd service for Thought Engine Daemon
echo "[5/5] Installing Thought Engine systemd service..."
cp "${REPO_DIR}/scripts/thought-engine.service" /etc/systemd/system/thought-engine.service
systemctl daemon-reload
systemctl enable thought-engine.service
systemctl start thought-engine.service

echo ""
echo "=== Setup Complete ==="
echo "KoboldCPP (Qwen3-8B) running on port ${KOBOLD_PORT}"
echo "Thought Engine Daemon running on port ${THOUGHT_ENGINE_PORT}"
echo "Check status: systemctl status thought-engine"
```

=== FILE: scripts/thought-engine.service ===
```ini
[Unit]
Description=CESAROPS Thought Engine Daemon
After=network.target koboldcpp.service
Wants=koboldcpp.service

[Service]
Type=simple
User=cesarops
Group=cesarops
WorkingDirectory=/home/cesarops/thought-engine
ExecStart=/home/cesarops/thought_engine_venv/bin/python /home/cesarops/thought-engine/thought_engine.py
Restart=always
RestartSec=10
StandardOutput=journal
StandardError=journal

# Environment
Environment=PYTHONUNBUFFERED=1
Environment=THOUGHT_ENGINE_PORT=5556
Environment=KOBOLD_BASE_URL=http://localhost:5555/v1
Environment=NAUTIVECS_URL=http://100.72.182.77:5003
Environment=CESAROPS_API_URL=http://100.72.182.77:5001

[Install]
WantedBy=multi-user.target
```

=== FILE: thought-engine/thought_engine.py ===
```python
"""
CESAROPS Thought Engine
=======================
A distributed reasoning system where a local 8B model plans/searches and a remote 35B model executes.

Architecture:
- Local: Qwen3-8B (KoboldCPP on :5555) for fast reasoning, planning, and verification.
- Remote: Qwen3.6-35B (on T440 :5001) for heavy code generation/analysis.
- Context: Nautivecs (:5003) for codebase retrieval.

Workflow:
1. User sends task to Thought Engine (FastAPI :5556).
2. 8B Model THINKS: Decomposes task, plans search strategy.
3. 8B Model SEARCHES: Queries Nautivecs (code) and Web (DuckDuckGo/scraping).
4. 8B Model PLANS: Generates structured spec for the 35B model.
5. Thought Engine DISPATCHES: Sends spec + context to 35B (T440).
6. 35B BUILDS: Produces implementation.
7. 8B Model VERIFIES: Checks output for correctness/syntax.
8. Result returned to user.
"""

import os
import sys
import json
import time
import logging
import asyncio
from typing import Optional, List, Dict, Any
from pathlib import Path
from dataclasses import dataclass, field

import httpx
from fastapi import FastAPI, HTTPException
from pydantic import BaseModel

# Configure Logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s [%(levelname)s] %(name)s: %(message)s",
    handlers=[
        logging.StreamHandler(sys.stdout),
        logging.FileHandler("thought_engine.log")
    ]
)
logger = logging.getLogger("ThoughtEngine")

# --- Configuration ---
KOBOLD_BASE_URL = os.getenv("KOBOLD_BASE_URL", "http://localhost:5555/v1")
NAUTIVECS_URL = os.getenv("NAUTIVECS_URL", "http://100.72.182.77:5003")
CESAROPS_API_URL = os.getenv("CESAROPS_API_URL", "http://100.72.182.77:5001")
THOUGHT_ENGINE_PORT = int(os.getenv("THOUGHT_ENGINE_PORT", "5556"))

# --- Pydantic Models ---

class TaskRequest(BaseModel):
    task_id: str = ""
    query: str
    context_hint: Optional[str] = None  # Optional hint for nautivecs search

class TaskResponse(BaseModel):
    task_id: str
    status: str  # 'processing', 'completed', 'failed'
    result: Optional[str] = None
    error: Optional[str] = None
    steps: List[Dict[str, Any]] = field(default_factory=list)

class SearchQuery(BaseModel):
    query: str
    source: str  # 'nautivecs' or 'web'

class PlanSpec(BaseModel):
    sub_tasks: List[Dict[str, str]]
    required_context: List[str]
    output_format: str

# --- Clients ---

class KoboldClient:
    """Client for local Qwen3-8B model."""
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.client = httpx.AsyncClient(base_url=base_url, timeout=120.0)

    async def chat_completion(self, messages: List[Dict[str, str]], temperature: float = 0.7, max_tokens: int = 4096) -> str:
        payload = {
            "model": "qwen3-8b",
            "messages": messages,
            "temperature": temperature,
            "max_tokens": max_tokens,
            "stream": False
        }
        try:
            resp = await self.client.post("/chat/completions", json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data["choices"][0]["message"]["content"]
        except httpx.HTTPError as e:
            logger.error(f"Kobold completion failed: {e}")
            raise

    async def health_check(self) -> bool:
        try:
            resp = await self.client.get("/health")
            return resp.status_code == 200
        except Exception:
            return False

    async def close(self):
        await self.client.aclose()

class NautivecsClient:
    """Client for remote Nautivecs codebase search."""
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.client = httpx.AsyncClient(base_url=base_url, timeout=30.0)

    async def search(self, query: str, limit: int = 5) -> List[Dict[str, Any]]:
        # Assuming Nautivecs has a /search endpoint returning JSON
        # Adjust endpoint based on actual Nautivecs API
        try:
            resp = await self.client.get("/search", params={"q": query, "limit": limit})
            resp.raise_for_status()
            return resp.json().get("results", [])
        except httpx.HTTPError as e:
            logger.warning(f"Nautivecs search failed for '{query}': {e}")
            return []

    async def close(self):
        await self.client.aclose()

class CesaropsClient:
    """Client for remote Qwen3.6-35B model (T440)."""
    def __init__(self, base_url: str):
        self.base_url = base_url
        self.client = httpx.AsyncClient(base_url=base_url, timeout=300.0) # Longer timeout for 35B

    async def execute_task(self, spec: PlanSpec, context: List[str]) -> str:
        # Construct prompt for the 35B model
        system_prompt = """You are CESAROPS, a powerful code generation and analysis engine.
        You receive structured tasks from a planning frontend.
        Your job is to produce high-quality, correct, and complete implementations.
        
        Input Format:
        - Sub-tasks: List of specific coding tasks.
        - Required Context: Code snippets or references to include.
        - Output Format: Expected structure of the result.
        
        Instructions:
        1. Follow the sub-tasks precisely.
        2. Use the provided context.
        3. Adhere strictly to the output format.
        4. If you encounter ambiguity, make reasonable assumptions and note them.
        """
        
        user_content = f"Sub-tasks:\n{json.dumps(spec.sub_tasks, indent=2)}\n\nRequired Context:\n{'---\n'.join(context)}\n\nOutput Format:\n{spec.output_format}"
        
        messages = [
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": user_content}
        ]
        
        try:
            payload = {
                "model": "qwen3.6-35b",
                "messages": messages,
                "temperature": 0.2, # Low temperature for deterministic code
                "max_tokens": 8192,
                "stream": False
            }
            resp = await self.client.post("/chat/completions", json=payload)
            resp.raise_for_status()
            data = resp.json()
            return data["choices"][0]["message"]["content"]
        except httpx.HTTPError as e:
            logger.error(f"Cesarops execution failed: {e}")
            raise

    async def close(self):
        await self.client.aclose()

class WebSearchClient:
    """Simple web scraper/searcher (placeholder for DuckDuckGo or similar)."""
    async def search(self, query: str) -> List[str]:
        # TODO: Integrate with actual DuckDuckGo scraper or SerpAPI
        # For now, return empty list
        logger.info(f"Web search requested for: {query}")
        return []

# --- Thought Engine Core ---

class ThoughtEngine:
    def __init__(self):
        self.kobold = KoboldClient(KOBOLD_BASE_URL)
        self.nautivecs = NautivecsClient(NAUTIVECS_URL)
        self.cesarops = CesaropsClient(CESAROPS_API_URL)
        self.web_search = WebSearchClient()
        self.tasks: Dict[str, TaskResponse] = {}

    async def process_task(self, request: TaskRequest) -> TaskResponse:
        task_id = request.task_id or f"task_{int(time.time())}"
        response = TaskResponse(task_id=task_id, status="processing")
        self.tasks[task_id] = response
        
        try:
            # Step 1: THINK - Decompose Query
            logger.info(f"[{task_id}] Step 1: Thinking/Decomposing...")
            thinking_prompt = [
                {"role": "system", "content": "You are a Research Lead. Break down this complex query into specific sub-tasks. Return JSON."},
                {"role": "user", "content": f"Query: {request.query}\n\nBreak this into 3-5 sub-tasks. Specify for each: task description, source (code/web), and rationale. Return as JSON array."}
            ]
            thinking_result = await self.kobold.chat_completion(thinking_prompt, temperature=0.7)
            response.steps.append({"step": "think", "output": thinking_result[:200] + "..."})
            
            # Parse JSON from thinking result
            try:
                plan_json = json.loads(thinking_result)
            except json.JSONDecodeError:
                # Fallback if LLM didn't return pure JSON
                plan_json = [{"task": request.query, "source": "code", "rationale": "Direct implementation"}]
            
            # Step 2: SEARCH - Gather Context
            logger.info(f"[{task_id}] Step 2: Searching...")
            context_list = []
            search_queries = [item.get("query", item.get("task", "")) for item in plan_json]
            
            # Search Nautivecs
            for sq in search_queries:
                results = await self.nautivecs.search(sq)
                for r in results:
                    context_list.append(f"Code Reference: {r.get('file', 'unknown')} -> {r.get('snippet', '')}")
            
            # Search Web
            for sq in search_queries:
                web_results = await self.web_search.search(sq)
                for wr in web_results:
                    context_list.append(f"Web Source: {wr}")
            
            response.steps.append({"step": "search", "context_count": len(context_list)})

            # Step 3: PLAN - Create Spec for 35B
            logger.info(f"[{task_id}] Step 3: Planning Spec...")
            planning_prompt = [
                {"role": "system", "content": "You are a Technical Planner. Convert research findings into a structured execution spec for a code generation engine."},
                {"role": "user", "content": f"Original Query: {request.query}\n\nSub-tasks from thinking: {json.dumps(plan_json)}\n\nGathered Context: {json.dumps(context_list[:5])}\n\nCreate a structured spec with: sub_tasks (list of dicts with 'description'), required_context (list of strings), and output_format (string). Return JSON."}
            ]
            plan_result = await self.kobold.chat_completion(planning_prompt, temperature=0.3)
            try:
                spec_json = json.loads(plan_result)
                spec = PlanSpec(
                    sub_tasks=spec_json.get("sub_tasks", []),
                    required_context=spec_json.get("required_context", []),
                    output_format=spec_json.get("output_format", "Code blocks with explanations")
                )
            except json.JSONDecodeError:
                # Fallback spec
                spec = PlanSpec(
                    sub_tasks=[{"description": request.query}],
                    required_context=context_list,
                    output_format="Code"
                )
            
            response.steps.append({"step": "plan", "spec_summary": str(spec)[:200]})

            # Step 4: DISPATCH - Send to 35B
            logger.info(f"[{task_id}] Step 4: Dispatching to 35B...")
            execution_result = await self.cesarops.execute_task(spec, context_list)
            response.steps.append({"step": "dispatch", "status": "completed"})

            # Step 5: VERIFY - Check Output
            logger.info(f"[{task_id}] Step 5: Verifying...")
            verification_prompt = [
                {"role": "system", "content": "You are a Code Reviewer. Check the following code for obvious errors, syntax issues, or logical flaws. If it looks good, say 'VERIFIED'. If not, explain why."},
                {"role": "user", "content": f"Original Task: {request.query}\n\nGenerated Code:\n{execution_result}"}
            ]
            verification_result = await self.kobold.chat_completion(verification_prompt, temperature=0.1)
            
            if "VERIFIED" in verification_result.upper() or "correct" in verification_result.lower():
                response.status = "completed"
                response.result = execution_result
            else:
                # Optional: Retry or flag for human review
                logger.warning(f"[{task_id}] Verification failed: {verification_result}")
                response.status = "completed_with_warnings"
                response.result = execution_result
                response.error = f"Verification warning: {verification_result[:100]}"
            
        except Exception as e:
            logger.exception(f"[{task_id}] Task failed: {e}")
            response.status = "failed"
            response.error = str(e)
        
        return response

# --- FastAPI App ---

app = FastAPI(title="CESAROPS Thought Engine", version="1.0.0")
engine = ThoughtEngine()

@app.on_event("startup")
async def startup():
    logger.info("Thought Engine starting up...")
    # Health checks
    if not await engine.kobold.health_check():
        logger.error("KoboldCPP (8B) is not reachable!")
    else:
        logger.info("KoboldCPP (8B) is reachable.")
    
    # Note: Nautivecs and Cesarops are remote, so we don't block on them here

@app.on_event("shutdown")
async def shutdown():
    await engine.kobold.close()
    await engine.nautivecs.close()
    await engine.cesarops.close()

@app.post("/tasks", response_model=TaskResponse)
async def create_task(request: TaskRequest):
    return await engine.process_task(request)

@app.get("/tasks/{task_id}", response_model=TaskResponse)
async def get_task(task_id: str):
    if task_id not in engine.tasks:
        raise HTTPException(status_code=404, detail="Task not found")
    return engine.tasks[task_id]

@app.get("/health")
async def health():
    return {"status": "ok", "kobold": await engine.kobold.health_check()}

if __name__ == "__main__":
    import uvicorn
    uvicorn.run(app, host="0.0.0.0", port=THOUGHT_ENGINE_PORT)
```

=== FILE: thought-engine/requirements.txt ===
```text
fastapi==0.109.0
uvicorn==0.27.0
httpx==0.26.0
pydantic==2.6.0
python-dotenv==1.0.0
```

=== FILE: scripts/test_thought_engine.py ===
```python
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
```