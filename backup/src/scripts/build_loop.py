"""Automated Build Loop — R1 (reviewer on 1070) drives the Strand (coder on P100s).

Flow:
1. Read BUILD_ORDER.md for the task list
2. For each file:
   a. Pre-fetch nautivecs context for the task
   b. Send task to Strand (P100s :5001) with context injection
   c. Send Strand's output to R1 (1070 :5555) for review
   d. If R1 finds errors → send corrections back to Strand (with nautivecs context)
   e. Write the final file to disk
   f. Run cargo check
   g. If errors → feed them back to Strand for fix
   h. Save lessons to nautivecs
3. Move to next file
"""
import json
import os
import subprocess
import sys
import time
import urllib.request

STRAND_URL = "http://127.0.0.1:5001/api/v1/generate"  # BF16 on P100s
R1_URL = "http://100.102.158.111:5555/api/v1/generate"  # 8B on 1070
NAUTIVECS_URL = "http://127.0.0.1:5003/query"
PROJECT_ROOT = "/codebase/wreckhunter2000-1"
INFERENCE_DIR = f"{PROJECT_ROOT}/cesarops-inference"
MAX_RETRIES = 3

# The build tasks in order
TASKS = [
    {
        "file": "src/loader.rs",
        "description": "GGUF model loader. Memory-maps GGUF file from RAID into GridBuffers. Parses GGUF header (magic 0x46475547, version 3). Extracts tensor metadata. Creates GridBuffer per tensor. Shards MoE experts across GPUs.",
        "search_terms": "GridBuffer from_raid_file GGUF loader memmap",
        "structs": "GgufLoader, TensorMeta { name, shape, offset, size, quant_type }, ModelWeights { layers, embedding, lm_head }, LayerWeights { attn_qkv, attn_out, ffn_gate, ffn_up, ffn_down, norm }",
        "deps": "memmap2::Mmap, warp_grid::mem::{GridBuffer, DeviceLocation}, warp_grid::types::{Precision, Error}, std::path::PathBuf, crate::hardware::IronProfile",
    },
    {
        "file": "src/bridge.rs",
        "description": "Zero-copy bridge between GridBuffer and Burn tensors. GridBuffer holds wgpu::Buffer handle. Burn-wgpu wraps same buffer without re-allocating. May require unsafe.",
        "search_terms": "GridBuffer burn tensor wgpu buffer zero-copy bridge",
        "structs": "No new structs. Functions: grid_to_burn_f16, grid_to_burn_f32",
        "deps": "burn, burn_wgpu, warp_grid::mem::GridBuffer, warp_grid::types::Precision",
    },
]


def fetch_nautivecs(query):
    """Pre-fetch context from nautivecs."""
    try:
        payload = json.dumps({"query": query, "top_k": 5}).encode()
        req = urllib.request.Request(NAUTIVECS_URL, data=payload, headers={"Content-Type": "application/json"})
        resp = json.loads(urllib.request.urlopen(req, timeout=10).read())
        parts = []
        for r in resp.get("results", [])[:5]:
            text = r.get("text", "")[:400]
            path = r.get("file_path", "")
            if text:
                parts.append(f"// {path}:\n{text}")
        return "\n\n".join(parts)
    except Exception as e:
        print(f"  [nautivecs: {e}]")
        return ""


def call_strand(prompt):
    """Generate code from the Strand on P100s."""
    payload = json.dumps({
        "prompt": prompt,
        "max_length": 4096,
        "temperature": 0.3,
        "top_p": 0.95,
        "rep_pen": 1.1,
        "stop_sequence": ["<|im_end|>", "```\n\n"],
    }).encode()
    req = urllib.request.Request(STRAND_URL, data=payload, headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req, timeout=300).read())
    return resp["results"][0]["text"]


def call_r1_review(code, task_desc):
    """Send code to R1 on 1070 for review."""
    prompt = f"""<|im_start|>system
You are a Rust code reviewer for the cesarops-inference crate. Review the code below.
If there are errors, list them concisely with corrections.
If the code is acceptable, respond with just: APPROVED
Focus on: wrong imports, wrong API signatures, missing error handling, logic errors.
Our crate uses warp_grid (NOT wasmcloud, NOT any cloud crate).
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
    req = urllib.request.Request(R1_URL, data=payload, headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req, timeout=120).read())
    return resp["results"][0]["text"]


def cargo_check():
    """Run cargo check and return errors."""
    result = subprocess.run(
        ["cargo", "check", "--message-format=short"],
        cwd=INFERENCE_DIR,
        capture_output=True, text=True, timeout=60,
        env={**os.environ, "PATH": f"/home/cesarops/.cargo/bin:{os.environ.get('PATH', '')}"}
    )
    if result.returncode == 0:
        return None
    return result.stderr[:2000]


def extract_rust_code(text):
    """Extract Rust code from markdown code blocks or raw text."""
    if "```rust" in text:
        start = text.index("```rust") + 7
        end = text.index("```", start) if "```" in text[start:] else len(text)
        return text[start:start + (end - start)].strip()
    elif "```" in text:
        start = text.index("```") + 3
        end = text.index("```", start) if "```" in text[start:] else len(text)
        return text[start:start + (end - start)].strip()
    return text.strip()


def save_lesson(lesson):
    """Append a lesson to the research log."""
    with open(f"{PROJECT_ROOT}/research_log/lessons_learned.md", "a") as f:
        f.write(f"\n## [build_loop,auto] {lesson}\n")


def build_file(task):
    """Build a single file through the Strand→R1→Strand loop."""
    file_path = task["file"]
    full_path = f"{INFERENCE_DIR}/{file_path}"
    print(f"\n{'='*60}")
    print(f"BUILDING: {file_path}")
    print(f"{'='*60}")

    # Step 1: Fetch nautivecs context
    print(f"  [1] Fetching nautivecs context for: {task['search_terms']}")
    context = fetch_nautivecs(task["search_terms"])
    print(f"      Got {len(context)} chars of context")

    # Step 2: Build prompt for Strand
    strand_prompt = f"""<|im_start|>system
You are the Fortytwo_Strand Rust Coder. Write production Rust code. Output ONLY the code, no explanations.

## CODEBASE CONTEXT (from nautivecs - these are REAL APIs, use them):
{context}
<|im_end|>
<|im_start|>user
Write {file_path} for the cesarops-inference crate.

Description: {task['description']}
Required structs: {task['structs']}
Required imports: {task['deps']}

Output ONLY valid Rust code. No markdown, no explanations.
<|im_end|>
<|im_start|>assistant
"""

    for attempt in range(1, MAX_RETRIES + 1):
        print(f"  [2] Strand generating (attempt {attempt})...")
        start = time.time()
        raw_output = call_strand(strand_prompt)
        elapsed = time.time() - start
        code = extract_rust_code(raw_output)
        print(f"      Generated {len(code)} chars in {elapsed:.1f}s")

        # Step 3: R1 reviews
        print(f"  [3] R1 reviewing...")
        review = call_r1_review(code, task["description"])
        print(f"      R1 says: {review[:200]}")

        if "APPROVED" in review.upper() or "acceptable" in review.lower():
            print(f"  [4] R1 APPROVED. Writing file.")
            os.makedirs(os.path.dirname(full_path), exist_ok=True)
            with open(full_path, "w") as f:
                f.write(code)
            break
        else:
            # Feed corrections back to Strand
            print(f"  [4] R1 found issues. Feeding corrections back...")
            strand_prompt = f"""<|im_start|>system
You are the Fortytwo_Strand Rust Coder. Fix the code based on the reviewer's corrections.
{context}
<|im_end|>
<|im_start|>user
Your previous code for {file_path} had errors:
{review}

Rewrite the file with these corrections applied. Output ONLY valid Rust code.
Required imports: {task['deps']}
<|im_end|>
<|im_start|>assistant
"""
            save_lesson(f"Strand error on {file_path} attempt {attempt}: {review[:200]}")
    else:
        print(f"  [!] Max retries reached. Writing best attempt.")
        os.makedirs(os.path.dirname(full_path), exist_ok=True)
        with open(full_path, "w") as f:
            f.write(code)

    # Step 5: Cargo check
    print(f"  [5] Running cargo check...")
    errors = cargo_check()
    if errors:
        print(f"      Cargo errors: {errors[:300]}")
        save_lesson(f"Cargo check failed on {file_path}: {errors[:200]}")
    else:
        print(f"      ✅ cargo check PASSED")

    print(f"  DONE: {file_path}")


if __name__ == "__main__":
    print("="*60)
    print("CESAROPS BUILD LOOP — Strand (P100s) + R1 (1070) + nautivecs")
    print("="*60)

    for task in TASKS:
        build_file(task)

    print("\n" + "="*60)
    print("ALL TASKS COMPLETE")
    print("="*60)
