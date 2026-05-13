#!/usr/bin/env python3
"""
The real test: Ask Qwen3.6-35B to design a unified mission control interface.
Full nautivecs context injection. This is the hardest task yet.
"""
import json
import urllib.request
import os

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/home/cesarops/wreckhunter2000-1/docs/mission_control_spec.md"

# Heavy context injection - pull everything relevant
print("=== Querying nautivecs for full system context ===")
queries = [
    "sovereign cloud API router pipeline dispatch node status",
    "thought engine process task dispatch verify plan",
    "nautivecs serve HTTP search query context injection",
    "cesarops hybrid engine cluster coordinator GPU mode flip",
    "research engine parallel fetch web oracle synthesis findings",
    "koboldcpp model swap service preloadstory contextsize",
    "scan worker satellite tile download process queue",
    "steering corrections feedback human loop",
]

all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:55]}] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:55]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)
print(f"\nTotal injected context: {len(combined_context)} chars from {len(all_context)} queries")

print("\n=== Sending mission control design request ===")

system_msg = f"""You are the CESAROPS research AI (Qwen3.6-35B-A3B MoE) running on dual Tesla P100 GPUs.

You are designing a system for YOUR OPERATOR — a dyslexic user who is a brilliant systems thinker but struggles with traditional coding interfaces. He built you as a scientific engine for shipwreck detection and search-and-rescue remote sensing. He needs a SIMPLE interface where he can:
- Click a button to switch between modes (coding, research, SAR missions)
- Talk to you naturally and have you execute complex pipelines
- See results visually without reading walls of code

Your existing codebase (retrieved via nautivecs — this is REAL code you can reference):

{combined_context}

You already have these working components:
- KoboldCPP serving you (Qwen3.6-35B) on port 5001 with OpenAI-compatible API
- nautivecs on port 5003 (2272 indexed code chunks, hybrid search)
- sovereign-cloud on port 8765 (cluster coordination, node discovery)
- cesarops-thought-engine (compiled, 8B reasons on 1070 → dispatches to you on P100s)
- cesarops-wso (web search oracle — DuckDuckGo scraper + Brave API)
- Cloudflare tunnel exposing everything at cesarops.org
- Steering system (.kiro/steering/) for persistent agent memory
- Weather-driven scan pipeline for shipwreck detection

The user mentioned "kowalski and moonweb" — he means a combination of:
- A smart backend orchestrator (like Kowalski from Penguins of Madagascar — "Kowalski, analysis!")
- A clean web UI (MoonWeb or similar Rust web framework) where he just clicks buttons

He wants ONE interface that can transform between:
1. CODING MODE: He describes what he wants, you write it, compile it, deploy it
2. CESAROPS MISSION MODE: Autonomous shipwreck scanning — weather monitoring, tile acquisition, anomaly detection, report generation
3. RESEARCH MODE: He asks a question, you search the web + codebase, synthesize findings, produce a paper or analysis

All running on his own hardware. No cloud. Accessible from school via Cloudflare tunnel."""

user_msg = """Design a unified Mission Control system for CESAROPS. The user is dyslexic and needs simplicity above all else. He should be able to:

1. Open a web page (app.cesarops.org)
2. See 3 big buttons: CODE | SCAN | RESEARCH
3. Click one, type or speak what he wants, and the system handles everything

## What I need you to design:

### Architecture
- How does this unify all existing components (thought-engine, nautivecs, WSO, sovereign-cloud, KoboldCPP)?
- What's the routing logic? How does "mode" change what happens to a user message?
- Can this be a single Rust binary with an embedded web UI (like how sovereign-cloud already has axum)?

### The Three Modes

**CODE mode:**
- User says: "Build a function that calculates tidal coefficients for Lake Michigan"
- System: thought-engine plans → nautivecs searches existing code → 35B generates → compiles → reports back
- UI shows: progress steps, final code, compile status, "Deploy?" button

**SCAN mode:**
- User says: "Run a scan on the Mackinac Straits area" or just clicks "Auto Scan"
- System: checks weather → selects optimal tiles → downloads imagery → runs GPU shaders → detects anomalies → generates report
- UI shows: map with scan area, weather status, detection confidence, anomaly markers

**RESEARCH mode:**
- User says: "What's the latest on using SAR for shallow water bathymetry?"
- System: thought-engine plans → WSO searches web → nautivecs searches codebase → 35B synthesizes → produces paper
- UI shows: sources found, synthesis progress, final document with citations

### UI Requirements (accessibility-first)
- Large buttons, high contrast, dyslexia-friendly fonts (OpenDyslexic or similar)
- Voice input option (browser speech-to-text API)
- Progress shown as visual steps (icons, not text walls)
- Results summarized in plain language first, details expandable
- Mobile-friendly (works on phone at school)

### Technical Implementation
- Should this be a new Rust crate? An extension of sovereign-cloud? A separate frontend?
- How does it talk to all the existing services?
- What's the minimal viable version we could deploy TODAY with what's already running?

### Preset System
- "Presets" = saved configurations for common tasks
- Examples: "Morning Scan" (check overnight weather, run any pending tiles), "Code Review" (pull latest git, analyze changes), "Research Brief" (summarize recent papers in our domain)
- Presets stored as JSON in nautivecs or on the external drive

Write a COMPLETE architecture spec. Include:
1. System diagram showing all component connections
2. API routes for the mission control server
3. The mode-switching logic
4. A minimal HTML/JS frontend concept (or Rust-based with Leptos/Yew)
5. What we can deploy TODAY vs what needs building
6. The preset system design

This is the capstone of the entire CESAROPS project. Make it worthy."""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 16384,
    "temperature": 0.6,
    "top_p": 0.95,
    "presence_penalty": 1.2
}

print(f"System: {len(system_msg)} chars | User: {len(user_msg)} chars")
print("Generating mission control spec (this is the big one — 15-20 min)...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=2400).read())
    content = resp["choices"][0]["message"]["content"]

    with open(OUTPUT, "w") as f:
        f.write(content)

    print(f"\n=== MISSION CONTROL SPEC GENERATED ===")
    print(f"Length: {len(content)} chars (~{len(content.split())} words)")
    print(f"Saved to: {OUTPUT}")
    print(f"\nFirst 500 chars:")
    print(content[:500])
    print("...")

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
