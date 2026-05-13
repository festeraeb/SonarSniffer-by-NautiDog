"""R1 Router — gives R1 hands (tools) without nudges.
Routes tool calls through nautivecs + WSO, returns clean results.
No corrections, no snark. Just execute and pass back.

Usage: python3 r1_router.py "Your question or task for R1"
"""
import json
import sys
import time
import urllib.request

R1_URL = "http://127.0.0.1:5001/api/v1/generate"
NAUTIVECS_URL = "http://127.0.0.1:5003/query"
WSO_URL = "http://127.0.0.1:5010/search"

MAX_ROUNDS = 8

SYSTEM = """You are DeepSeek-R1, a deep reasoning AI with access to tools.

## Available Tools
Call tools using this exact format:
<tool_call>
{"name": "tool_name", "arguments": {"key": "value"}}
</tool_call>

### Tools:
- **think_harder**: Search the nautivecs knowledge base (12,600+ code chunks) + web search. Args: {"query": "search terms"}
- **search_web**: Search the internet via SearXNG. Args: {"query": "search terms", "max_results": 5}
- **search_code**: Search the indexed codebase for specific code/functions. Args: {"query": "function or concept"}

## Rules:
1. ALWAYS search before reasoning. The knowledge base knows more than you.
2. Call ONE tool at a time.
3. After receiving results, either search again or provide your final answer.
4. Be thorough. Take your time. This is research, not chat.
5. IMPORTANT: Answer ONE SECTION at a time. After each section, STOP with [SECTION COMPLETE].
   The system will prompt you for the next section."""


def prefetch_nautivecs(task_description):
    """Pre-fetch relevant code context from nautivecs before generating."""
    # Extract key terms from the task
    keywords = task_description[:200]  # Use first 200 chars as query
    
    try:
        payload = json.dumps({"query": keywords, "top_k": 5, "include_context": True}).encode()
        req = urllib.request.Request(NAUTIVECS_URL, data=payload,
                                   headers={"Content-Type": "application/json"})
        resp = json.loads(urllib.request.urlopen(req, timeout=10).read())
        
        context_parts = []
        if resp.get("results"):
            for r in resp["results"][:5]:
                file_path = r.get("file_path", "")
                func_name = r.get("function_name", "")
                text = r.get("text", "")[:500]
                score = r.get("score", 0)
                if score > 0.1:
                    context_parts.append(f"// From {file_path} ({func_name}):\n{text}")
        
        if context_parts:
            return "\n\n".join(context_parts)
    except Exception as e:
        print(f"  [nautivecs prefetch failed: {e}]")
    
    return ""


def execute_tool(name, arguments):
    """Execute a tool call and return the result string."""
    if name == "think_harder":
        query = arguments.get("query", "")
        results = []
        # Hit nautivecs
        try:
            payload = json.dumps({"query": query, "top_k": 5}).encode()
            req = urllib.request.Request(NAUTIVECS_URL, data=payload,
                                       headers={"Content-Type": "application/json"})
            resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
            if resp.get("results"):
                for r in resp["results"][:3]:
                    results.append(f"[nautivecs {r.get('score',0):.2f}] {r.get('file_path','')}: {r.get('text','')[:300]}")
        except Exception as e:
            results.append(f"[nautivecs error: {e}]")
        # Hit WSO
        try:
            payload = json.dumps({"query": query, "max_results": 3}).encode()
            req = urllib.request.Request(WSO_URL, data=payload,
                                       headers={"Content-Type": "application/json"})
            resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
            if isinstance(resp, list):
                for r in resp[:3]:
                    results.append(f"[web] {r.get('title','')}: {r.get('snippet','')[:200]}")
            elif resp.get("results"):
                for r in resp["results"][:3]:
                    results.append(f"[web] {r.get('title','')}: {r.get('snippet','')[:200]}")
        except Exception as e:
            results.append(f"[web search error: {e}]")
        return "\n".join(results) if results else "No results found."

    elif name == "search_web":
        query = arguments.get("query", "")
        max_results = arguments.get("max_results", 5)
        try:
            payload = json.dumps({"query": query, "max_results": max_results}).encode()
            req = urllib.request.Request(WSO_URL, data=payload,
                                       headers={"Content-Type": "application/json"})
            resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
            results = []
            items = resp if isinstance(resp, list) else resp.get("results", [])
            for r in items[:max_results]:
                results.append(f"- {r.get('title','')}: {r.get('snippet','')[:300]}")
            return "\n".join(results) if results else "No web results."
        except Exception as e:
            return f"Web search error: {e}"

    elif name == "search_code":
        query = arguments.get("query", "")
        try:
            payload = json.dumps({"query": query, "top_k": 5}).encode()
            req = urllib.request.Request(NAUTIVECS_URL, data=payload,
                                       headers={"Content-Type": "application/json"})
            resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
            results = []
            for r in resp.get("results", [])[:5]:
                results.append(f"[{r.get('score',0):.2f}] {r.get('file_path','')}:{r.get('line_start',0)} ({r.get('function_name','')})\n{r.get('text','')[:400]}")
            return "\n---\n".join(results) if results else "No code matches."
        except Exception as e:
            return f"Code search error: {e}"

    return f"Unknown tool: {name}"


def parse_tool_call(text):
    """Extract tool call from model output."""
    import re
    match = re.search(r'<tool_call>\s*(.*?)\s*</tool_call>', text, re.DOTALL)
    if match:
        try:
            data = json.loads(match.group(1))
            return data.get("name"), data.get("arguments", {})
        except json.JSONDecodeError:
            # Try regex extraction
            name_match = re.search(r'"name"\s*:\s*"([^"]+)"', match.group(1))
            args_match = re.search(r'"arguments"\s*:\s*(\{.*\})', match.group(1), re.DOTALL)
            if name_match:
                name = name_match.group(1)
                args = json.loads(args_match.group(1)) if args_match else {}
                return name, args
    return None, None


def generate(prompt, max_length=8192):
    """Call R1 and return generated text."""
    payload = json.dumps({
        "prompt": prompt,
        "max_length": max_length,
        "temperature": 0.7,
        "top_p": 0.95,
        "rep_pen": 1.1,
        "stop_sequence": ["</tool_call>", "<|im_end|>", "[SECTION COMPLETE]"],
    }).encode()
    req = urllib.request.Request(R1_URL, data=payload,
                               headers={"Content-Type": "application/json"})
    resp = json.loads(urllib.request.urlopen(req, timeout=3600).read())  # 1 hour timeout
    text = resp["results"][0]["text"]
    # If stopped at </tool_call>, re-append it
    if "<tool_call>" in text and "</tool_call>" not in text:
        text += "</tool_call>"
    return text


def run(user_message):
    """Main router loop — R1 with hands."""
    messages = []
    
    # MANDATORY: Pre-fetch nautivecs context before generating
    prefetch_context = prefetch_nautivecs(user_message)
    
    system_with_context = SYSTEM
    if prefetch_context:
        system_with_context += f"\n\n## CODEBASE CONTEXT (from nautivecs - USE THIS):\n{prefetch_context}\n"
    
    prompt = f"<|im_start|>system\n{system_with_context}<|im_end|>\n<|im_start|>user\n{user_message}<|im_end|>\n<|im_start|>assistant\n"

    tool_log = []

    for round_num in range(1, MAX_ROUNDS + 1):
        print(f"\n[Round {round_num}] Generating...")
        start = time.time()
        output = generate(prompt, max_length=1024)
        elapsed = time.time() - start
        print(f"  Generated {len(output)} chars in {elapsed:.1f}s")

        # Check for tool call
        tool_name, tool_args = parse_tool_call(output)

        if tool_name:
            print(f"  Tool call: {tool_name}({json.dumps(tool_args)[:80]})")
            result = execute_tool(tool_name, tool_args)
            tool_log.append(f"{tool_name}: {json.dumps(tool_args).get('query', '')[:50] if isinstance(tool_args, dict) else ''}")
            print(f"  Result: {result[:200]}...")

            # Append to prompt as user message (clean, no nudges)
            prompt += output + f"\n<|im_end|>\n<|im_start|>user\n[Tool Result]: {result}\n<|im_end|>\n<|im_start|>assistant\n"
        else:
            # Final answer — no tool call
            print(f"\n{'='*60}")
            print("R1 FINAL ANSWER:")
            print(f"{'='*60}")
            print(output)
            print(f"{'='*60}")
            print(f"\nTools used: {' -> '.join(tool_log)}")

            # Save to file
            out_path = "/codebase/wreckhunter2000-1/docs/r1_answers/latest.md"
            with open(out_path, "w") as f:
                f.write(f"# R1 Research Output\n\n")
                f.write(f"**Question:** {user_message}\n\n")
                f.write(f"**Tools used:** {' -> '.join(tool_log)}\n\n")
                f.write(f"**Rounds:** {round_num}\n\n---\n\n")
                f.write(output)
            print(f"\nSaved to: {out_path}")
            return output

    print("[WARNING] Exhausted max rounds without final answer")
    return output


if __name__ == "__main__":
    if len(sys.argv) < 2:
        print("Usage: python3 r1_router.py \"Your question\"")
        sys.exit(1)

    question = " ".join(sys.argv[1:])
    run(question)
