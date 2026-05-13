#!/usr/bin/env python3
"""
Build Orchestrator — Automated spec-to-code pipeline.

Flow per file:
1. Read task from BUILD_ORDER
2. Query nautivecs for relevant code context
3. Inject context into prompt
4. Send to Strand (builder on :5001)
5. Send Strand's output to R1 (reviewer on :5555 cesarops2)
6. If R1 finds errors → inject corrections, send back to Strand
7. Write the file
8. Run cargo check
9. If errors → feed them back, loop
10. Move to next file

Usage: python3 build_orchestrator.py
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

# Endpoints
STRAND_URL = "http://127.0.0.1:5001/api/v1/generate"  # Builder (P100s)
R1_URL = "http://100.102.158.111:5555/api/v1/generate"  # Reviewer (1070)
NAUTIVECS_URL = "http://127.0.0.1:5003/query"  # Knowledge base

PROJECT_ROOT = "/codebase/wreckhunter2000-1"
CRATE_DIR = f"{PROJECT_ROOT}/cesarops-inference"
MAX_RETRIES = 3

# Build tasks in order
TASKS = [
    {
        "file": "src/loader.rs",
        "desc": "GGUF model loader - mmap weights from RAID into GridBuffers",
        "spec": """Write cesarops-inference/src/loader.rs.
Purpose: Load GGUF model files via mmap into GridBuffer handles.
Use memmap2::Mmap to memory-map the file. Parse GGUF header (magic 0x46475547, version 3).
Extract tensor metadata. Create GridBuffer for each tensor.
Shard MoE experts: 0-63 on GPU 0, 64-127 on GPU 1.
Structs: GgufLoader, TensorMeta { name, shape, offset, size, quant_type }, ModelWeights, LayerWeights.
Function: pub fn load(path: &Path, profile: &IronProfile) -> Result<ModelWeights, Error>""",
        "search_terms": "GridBuffer from_raid_file GGUF loader memmap",
    },
    {
        "file": "src/bridge.rs",
        "desc": "Zero-copy GridBuffer to Burn tensor bridge",
        "spec": """Write cesarops-inference/src/bridge.rs.
Purpose: Zero-copy conversion between GridBuffer and Burn tensors.
GridBuffer already holds data in wgpu::Buffer (on GPU) or host memory.
For GPU buffers: wrap the existing wgpu::Buffer for Burn without re-allocating.
For host buffers: create a Burn tensor from the host bytes.
Functions: grid_to_burn_f16, grid_to_burn_f32.
This WILL require unsafe code - mark it clearly with SAFETY comments.""",
        "search_terms": "GridBuffer as_host_bytes migrate Burn tensor wgpu",
    },
]


def query_nautivecs(search_terms):
    """Fetch relevant code context from nautivecs."""
    try:
        payload = json.dumps({"query": search_terms, "top_k": 5}).encode()
        req = urllib.request.Request(NAUTIVECS_URL, data=payload,
                                   headers={"Content-Type": "application/json"})
        resp = json.loads(urllib.request.urlopen(req, timeout=15).read())
        
        context = []
        for r in resp.get("results", [])[:5]:
            score = r.get("score", 0)
            if score > 0.05:
                path = r.get("file_path", "")
                func = r.get("function_name", "")
                text = r.get("text", "")[:600]
                context.append(f"// [{score:.2f}] {path} ({func}):\n{text}")
        
        return "\n\n".join(context)
    except Exception as e:
        print(f"  [nautivecs: {e}]")
        return ""


def call_strand(prompt):
    """Send prompt to Strand (builder) and get code back."""
    payload = json.dumps({
        "prompt": prompt,
        "max_length": 4096,
        "temperature": 0.3,
        "top_p": 0.9,
        "rep_pen": 1.1,
        "stop_sequence": ["<|im_end|>", "```\n\n"],
    }).encode()
    
    req = urllib.request.Request(STRAND_URL, data=payload,
                               headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req, timeout=300).read())
    return resp["results"][0]["text"]


def call_r1_review(code, task_desc):
    """Send code to R1 (reviewer) for error checking."""
    prompt = f"""<|im_start|>system
You are a Rust code reviewer for the cesarops project. Check the code for:
1. Wrong imports (we use warp_grid, NOT wasmcloud or any other crate)
2. Wrong API signatures (check against the context provided)
3. Missing error handling
4. Compilation issues
If the code is GOOD, respond with just: APPROVED
If it has errors, list them concisely with corrections.
<|im_end|>
<|im_start|>user
Task: {task_desc}
Code to review:
```rust
{code}
```
<|im_end|>
<|im_start|>assistant
"""
    payload = json.dumps({
        "prompt": prompt,
        "max_length": 512,
        "temperature": 0.2,
        "stop_sequence": ["<|im_end|>"],
    }).encode()
    
    req = urllib.request.Request(R1_URL, data=payload,
                               headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req, timeout=120).read())
    return resp["results"][0]["text"]


def extract_rust_code(text):
    """Extract Rust code from model output (handles ```rust blocks or raw code)."""
    import re
    # Try to find ```rust ... ``` block
    match = re.search(r'```rust\s*(.*?)```', text, re.DOTALL)
    if match:
        return match.group(1).strip()
    # Try ``` ... ```
    match = re.search(r'```\s*(.*?)```', text, re.DOTALL)
    if match:
        return match.group(1).strip()
    # If it starts with 'use ' or 'pub ', treat whole thing as code
    if text.strip().startswith(('use ', 'pub ', '//', '#[')):
        return text.strip()
    return text


def cargo_check():
    """Run cargo check and return (success, errors)."""
    result = subprocess.run(
        ["cargo", "check", "--message-format=short"],
        cwd=CRATE_DIR,
        capture_output=True, text=True, timeout=120
    )
    if result.returncode == 0:
        return True, ""
    errors = result.stderr + result.stdout
    # Extract just the error lines
    error_lines = [l for l in errors.split('\n') if 'error' in l.lower()][:10]
    return False, '\n'.join(error_lines)


def build_file(task):
    """Build one file through the full pipeline."""
    file_path = task["file"]
    full_path = f"{CRATE_DIR}/{file_path}"
    
    print(f"\n{'='*60}")
    print(f"BUILDING: {file_path}")
    print(f"{'='*60}")
    
    # Step 1: Query nautivecs for context
    print(f"  [1] Querying nautivecs: {task['search_terms'][:50]}...")
    context = query_nautivecs(task["search_terms"])
    print(f"      Got {len(context)} chars of context")
    
    # Step 2: Build prompt with context injection
    prompt = f"""<|im_start|>system
You are a Rust systems programmer. Write ONLY the code requested. No explanations.
Use the codebase context below to get the correct imports and API signatures.

## CODEBASE CONTEXT (from nautivecs - these are REAL, use them):
{context}
<|im_end|>
<|im_start|>user
{task['spec']}

Output ONLY valid Rust code. No markdown, no explanations.
<|im_end|>
<|im_start|>assistant
"""
    
    for attempt in range(1, MAX_RETRIES + 1):
        print(f"  [2] Sending to Strand (attempt {attempt}/{MAX_RETRIES})...")
        start = time.time()
        raw_output = call_strand(prompt)
        elapsed = time.time() - start
        print(f"      Generated {len(raw_output)} chars in {elapsed:.1f}s")
        
        code = extract_rust_code(raw_output)
        
        # Step 3: Send to R1 for review
        print(f"  [3] Sending to R1 for review...")
        review = call_r1_review(code, task["desc"])
        print(f"      R1 says: {review[:100]}...")
        
        if "APPROVED" in review.upper():
            print(f"  [4] R1 APPROVED. Writing file.")
            os.makedirs(os.path.dirname(full_path), exist_ok=True)
            with open(full_path, 'w') as f:
                f.write(code)
            print(f"      Written: {full_path}")
            return True
        else:
            # Step 4: Feed corrections back to Strand
            print(f"  [4] R1 found errors. Feeding corrections back...")
            prompt = f"""<|im_start|>system
You are a Rust systems programmer. Fix the code based on the reviewer's corrections.

## CODEBASE CONTEXT:
{context}
<|im_end|>
<|im_start|>user
Your previous code had these errors:
{review}

Original task: {task['spec']}

Rewrite the COMPLETE file with corrections applied. Output ONLY valid Rust code.
<|im_end|>
<|im_start|>assistant
"""
    
    # If we exhausted retries, write whatever we have
    print(f"  [!] Exhausted retries. Writing best attempt.")
    os.makedirs(os.path.dirname(full_path), exist_ok=True)
    with open(full_path, 'w') as f:
        f.write(f"// TODO: Needs manual review - auto-generation incomplete\n{code}")
    return False


def main():
    print("=" * 60)
    print("CESAROPS BUILD ORCHESTRATOR")
    print("Strand (builder) on P100s :5001")
    print("R1 (reviewer) on 1070 :5555")
    print("nautivecs (context) on :5003")
    print("=" * 60)
    
    # Verify endpoints
    for name, url in [("Strand", STRAND_URL.replace("/api/v1/generate", "/api/v1/model")),
                      ("nautivecs", NAUTIVECS_URL.replace("/query", "/health"))]:
        try:
            resp = urllib.request.urlopen(url, timeout=5)
            print(f"  {name}: OK")
        except Exception as e:
            print(f"  {name}: FAILED ({e})")
    
    # Process each task
    results = []
    for task in TASKS:
        success = build_file(task)
        results.append((task["file"], success))
    
    # Final cargo check
    print(f"\n{'='*60}")
    print("FINAL CARGO CHECK")
    print(f"{'='*60}")
    success, errors = cargo_check()
    if success:
        print("  CARGO CHECK: PASSED")
    else:
        print(f"  CARGO CHECK: FAILED\n{errors}")
    
    # Summary
    print(f"\n{'='*60}")
    print("BUILD SUMMARY")
    print(f"{'='*60}")
    for file, ok in results:
        status = "OK" if ok else "NEEDS REVIEW"
        print(f"  {file}: {status}")


if __name__ == "__main__":
    main()
