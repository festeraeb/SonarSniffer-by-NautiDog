#!/usr/bin/env python3
"""
CESAROPS Overnight Research Agent v2
======================================
Two-stage research pipeline:
  Stage 1 (OpenScholar-8B): Literature search & synthesis
  Stage 2 (ChatGLM3/SciGLM): Physics reasoning & spec generation

Modes:
  --research    : Search papers, synthesize, produce specs
  --scan        : Blind wreck scanning (probe & learn)
  --both        : Split time between research and scanning
  --once        : Single research pass then exit
  --report      : Show accumulated findings

Models:
  OpenScholar-8B (Q6_K) — literature gathering & synthesis
  ChatGLM3-6B (Q6_K)   — scientific reasoning & spec writing
  Both run via koboldcpp on local GPUs.

Environment:
    SCHOLAR_URL         — OpenScholar endpoint (default: http://127.0.0.1:5020)
    SCIGLM_URL          — SciGLM/ChatGLM3 endpoint (default: http://127.0.0.1:5021)
    KOBOLD_URL          — Fallback LLM endpoint (default: http://100.72.182.77:5001)
    RESEARCH_INTERVAL   — minutes between passes (default: 30)
    RESEARCH_MAX_PAPERS — max papers per topic per pass (default: 5)
"""

import argparse
import json
import os
import sys
import time
import urllib.request
import urllib.parse
from datetime import datetime, timezone, date
from pathlib import Path
from typing import Dict, List, Optional

REPO = Path(__file__).resolve().parent
LOG_DIR = REPO / "research_log"
LOG_DIR.mkdir(parents=True, exist_ok=True)
SPECS_DIR = REPO / "research_log" / "specs"
SPECS_DIR.mkdir(parents=True, exist_ok=True)

# ── Config ────────────────────────────────────────────────────────────────────

def _load_env() -> dict:
    env = {}
    p = REPO / ".env"
    if p.exists():
        for line in p.read_text(encoding='utf-8').splitlines():
            line = line.strip()
            if line and not line.startswith('#') and '=' in line:
                k, _, v = line.partition('=')
                env[k.strip()] = v.strip()
    return env

_ENV = _load_env()

def _cfg(key: str, default: str = "") -> str:
    return os.environ.get(key, _ENV.get(key, default))

SCHOLAR_URL = _cfg("SCHOLAR_URL", "http://127.0.0.1:5020")
SCIGLM_URL = _cfg("SCIGLM_URL", "http://127.0.0.1:5021")
KOBOLD_URL = _cfg("KOBOLD_URL", "http://100.72.182.77:5001")
RESEARCH_INTERVAL = int(_cfg("RESEARCH_INTERVAL", "30"))
MAX_PAPERS = int(_cfg("RESEARCH_MAX_PAPERS", "5"))

# ── Research Topics ───────────────────────────────────────────────────────────

RESEARCH_TOPICS = [
    "curvelet transform sub-surface anomaly detection sonar bathymetry",
    "aeromagnetic dipole anomaly detection shipwreck ferrous",
    "satellite SAR ship detection spectral analysis Sentinel-2",
    "temporal stacking change detection remote sensing water",
    "Richardson number oceanographic layering thermocline detection",
    "wgpu Vulkan GPU compute geospatial raster processing",
    "HBM2 memory bandwidth optimization GPU inference",
    "subpixel registration satellite imagery alignment drift",
    "GeoTIFF coordinate reference system precision alignment",
    "multi-temporal SAR coherence shipwreck Great Lakes",
]

# ── LLM Interface ─────────────────────────────────────────────────────────────

def query_llm(prompt: str, url: str, max_tokens: int = 4096, temperature: float = 0.3) -> str:
    """Send a prompt to a koboldcpp-compatible endpoint."""
    payload = json.dumps({
        "prompt": prompt,
        "max_length": max_tokens,
        "temperature": temperature,
        "top_p": 0.9,
        "rep_pen": 1.1,
        "stop_sequence": ["<|im_end|>", "<|endoftext|>", "\n\n---\n"],
    }).encode('utf-8')

    req = urllib.request.Request(
        f"{url}/api/v1/generate",
        data=payload,
        headers={"Content-Type": "application/json"},
    )

    try:
        with urllib.request.urlopen(req, timeout=600) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            results = data.get("results", [])
            if results:
                return results[0].get("text", "").strip()
            return ""
    except Exception as e:
        print(f"  [LLM ERROR] {url}: {e}")
        return ""

# ── Paper Search (Crossref + Semantic Scholar) ────────────────────────────────

def search_crossref(query: str, max_results: int = 5) -> List[Dict]:
    """Search Crossref for recent papers."""
    encoded = urllib.parse.quote(query)
    url = f"https://api.crossref.org/works?query={encoded}&rows={max_results}&sort=published&order=desc"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "CesarOps/1.0 (research-agent)"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            items = data.get("message", {}).get("items", [])
            results = []
            for item in items:
                results.append({
                    "title": " ".join(item.get("title", ["Untitled"])),
                    "doi": item.get("DOI", ""),
                    "abstract": item.get("abstract", "")[:500] if item.get("abstract") else "",
                    "year": item.get("published", {}).get("date-parts", [[0]])[0][0],
                    "source": "crossref",
                })
            return results
    except Exception as e:
        print(f"  [CROSSREF ERROR] {e}")
        return []

def search_semantic_scholar(query: str, max_results: int = 5) -> List[Dict]:
    """Search Semantic Scholar for papers with abstracts."""
    encoded = urllib.parse.quote(query)
    url = f"https://api.semanticscholar.org/graph/v1/paper/search?query={encoded}&limit={max_results}&fields=title,abstract,year,citationCount"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "CesarOps/1.0"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            data = json.loads(resp.read().decode('utf-8'))
            papers = data.get("data", [])
            results = []
            for p in papers:
                results.append({
                    "title": p.get("title", "Untitled"),
                    "abstract": (p.get("abstract") or "")[:500],
                    "year": p.get("year", 0),
                    "citations": p.get("citationCount", 0),
                    "source": "semantic_scholar",
                })
            return results
    except Exception as e:
        print(f"  [S2 ERROR] {e}")
        return []

# ── Stage 1: OpenScholar — Literature Synthesis ───────────────────────────────

def stage1_gather_and_synthesize(topic: str) -> Optional[Dict]:
    """Use OpenScholar to search papers and synthesize findings."""
    print(f"\n  [STAGE 1] Gathering literature: {topic[:60]}...")

    # Search for papers
    papers = search_crossref(topic, MAX_PAPERS)
    papers += search_semantic_scholar(topic, MAX_PAPERS)

    if not papers:
        print("    No papers found.")
        return None

    # Deduplicate by title similarity
    seen_titles = set()
    unique_papers = []
    for p in papers:
        title_key = p["title"].lower()[:50]
        if title_key not in seen_titles:
            seen_titles.add(title_key)
            unique_papers.append(p)

    # Format papers for the LLM
    paper_text = ""
    for i, p in enumerate(unique_papers[:8], 1):
        paper_text += f"\n{i}. \"{p['title']}\" ({p.get('year', '?')})\n"
        if p.get("abstract"):
            paper_text += f"   Abstract: {p['abstract']}\n"

    # Ask OpenScholar to synthesize
    synthesis_prompt = f"""<|im_start|>system
You are a scientific literature synthesis agent. Your job is to read paper abstracts and produce a concise synthesis of the key findings, methods, and open questions relevant to maritime search and rescue technology.<|im_end|>
<|im_start|>user
Topic: {topic}

Papers found:
{paper_text}

Synthesize these papers into:
1. KEY FINDINGS: What methods work? What accuracy/performance do they achieve?
2. RELEVANT METHODS: Which techniques could apply to our shipwreck detection pipeline?
3. OPEN QUESTIONS: What hasn't been solved yet?
4. ACTIONABLE IDEAS: Specific things we could implement based on these findings.

Be concise and technical.<|im_end|>
<|im_start|>assistant
"""

    synthesis = query_llm(synthesis_prompt, SCHOLAR_URL, max_tokens=2048)

    if not synthesis:
        # Fallback to main LLM
        synthesis = query_llm(synthesis_prompt, KOBOLD_URL, max_tokens=2048)

    if not synthesis:
        print("    Synthesis failed.")
        return None

    print(f"    Synthesized {len(unique_papers)} papers → {len(synthesis)} chars")

    return {
        "topic": topic,
        "papers": unique_papers,
        "synthesis": synthesis,
        "timestamp": datetime.now(timezone.utc).isoformat(),
    }

# ── Stage 2: SciGLM/ChatGLM3 — Spec Generation ──────────────────────────────

def stage2_generate_spec(finding: Dict) -> Optional[str]:
    """Use ChatGLM3 (SciGLM base) to reason through findings and produce a spec."""
    print(f"  [STAGE 2] Generating spec from: {finding['topic'][:50]}...")

    spec_prompt = f"""You are a scientific reasoning agent specializing in physics, signal processing, and remote sensing. Given research findings, produce an implementation specification.

Research Topic: {finding['topic']}

Literature Synthesis:
{finding['synthesis'][:2000]}

Based on these findings, write a SPECIFICATION for implementing the most promising technique in our Rust/wgpu pipeline. Include:

1. ALGORITHM: Step-by-step description of the method
2. INPUTS: What data format and resolution is needed
3. OUTPUTS: What the result looks like
4. MATH: Key equations (use plain text notation)
5. GPU CONSIDERATIONS: Memory requirements, parallelization strategy
6. VALIDATION: How to verify correctness
7. INTEGRATION: How this fits into our temporal stacking pipeline

Write as a technical spec, not a paper summary."""

    spec = query_llm(spec_prompt, SCIGLM_URL, max_tokens=4096, temperature=0.2)

    if not spec:
        # Fallback to main LLM
        spec = query_llm(spec_prompt, KOBOLD_URL, max_tokens=4096, temperature=0.2)

    if not spec:
        print("    Spec generation failed.")
        return None

    print(f"    Spec generated: {len(spec)} chars")
    return spec

# ── Main Research Loop ────────────────────────────────────────────────────────

def run_research_pass() -> List[Dict]:
    """Run one full research pass: gather → synthesize → spec."""
    session_results = []
    today = date.today().isoformat()

    for topic in RESEARCH_TOPICS:
        # Stage 1: Gather & synthesize
        finding = stage1_gather_and_synthesize(topic)
        if not finding:
            continue

        # Stage 2: Generate spec
        spec = stage2_generate_spec(finding)
        if spec:
            finding["spec"] = spec

            # Save spec to file
            safe_topic = topic.replace(" ", "_")[:40]
            spec_file = SPECS_DIR / f"{today}_{safe_topic}.md"
            spec_content = f"# Spec: {topic}\n\n"
            spec_content += f"Generated: {finding['timestamp']}\n\n"
            spec_content += f"## Literature Synthesis\n\n{finding['synthesis']}\n\n"
            spec_content += f"## Implementation Specification\n\n{spec}\n"
            spec_file.write_text(spec_content, encoding='utf-8')
            print(f"    Saved: {spec_file.name}")

        session_results.append(finding)

        # Rate limit between topics
        time.sleep(5)

    # Save session log
    log_file = LOG_DIR / f"{today}_session.json"
    existing = []
    if log_file.exists():
        try:
            existing = json.loads(log_file.read_text(encoding='utf-8'))
        except Exception:
            pass
    existing.extend(session_results)
    log_file.write_text(json.dumps(existing, indent=2, ensure_ascii=False), encoding='utf-8')

    return session_results

def run_overnight_loop(mode: str = "research"):
    """Main overnight loop with mode selection."""
    print(f"\n{'='*60}")
    print(f"  CESAROPS Overnight Research Agent v2")
    print(f"  Mode: {mode}")
    print(f"  Scholar: {SCHOLAR_URL}")
    print(f"  SciGLM:  {SCIGLM_URL}")
    print(f"  Fallback: {KOBOLD_URL}")
    print(f"  Interval: {RESEARCH_INTERVAL} min")
    print(f"{'='*60}\n")

    pass_count = 0
    while True:
        pass_count += 1
        start = time.time()
        print(f"\n--- Pass {pass_count} @ {datetime.now().strftime('%H:%M:%S')} ---")

        if mode == "research":
            results = run_research_pass()
            print(f"  Pass complete: {len(results)} topics processed")

        elif mode == "scan":
            # Placeholder for blind scanning mode
            print("  [SCAN MODE] Blind wreck scanning — not yet wired to new pipeline")
            # TODO: Wire to idle_scout / scan_worker

        elif mode == "both":
            # Split: research first half, scan second half
            half_topics = RESEARCH_TOPICS[:len(RESEARCH_TOPICS)//2]
            original = RESEARCH_TOPICS.copy()
            RESEARCH_TOPICS.clear()
            RESEARCH_TOPICS.extend(half_topics)
            results = run_research_pass()
            RESEARCH_TOPICS.clear()
            RESEARCH_TOPICS.extend(original)
            print(f"  Research half: {len(results)} topics")
            print("  [SCAN MODE] Blind scanning half — not yet wired")

        elapsed = time.time() - start
        sleep_time = max(0, RESEARCH_INTERVAL * 60 - elapsed)
        print(f"  Elapsed: {elapsed:.0f}s. Sleeping {sleep_time:.0f}s until next pass...")
        time.sleep(sleep_time)

def show_report():
    """Show accumulated research findings."""
    print("\n=== CESAROPS Research Report ===\n")
    spec_files = sorted(SPECS_DIR.glob("*.md"))
    if not spec_files:
        print("No specs generated yet.")
        return
    for f in spec_files:
        print(f"  {f.name}")
    print(f"\n  Total: {len(spec_files)} specs")
    print(f"  Location: {SPECS_DIR}/")

# ── CLI ───────────────────────────────────────────────────────────────────────

if __name__ == "__main__":
    parser = argparse.ArgumentParser(description="CESAROPS Overnight Research Agent v2")
    parser.add_argument("--research", action="store_true", help="Research mode (default)")
    parser.add_argument("--scan", action="store_true", help="Blind scanning mode")
    parser.add_argument("--both", action="store_true", help="Split time: research + scan")
    parser.add_argument("--once", action="store_true", help="Single pass then exit")
    parser.add_argument("--report", action="store_true", help="Show findings report")
    args = parser.parse_args()

    if args.report:
        show_report()
    elif args.once:
        results = run_research_pass()
        print(f"\nDone. {len(results)} topics processed.")
    elif args.scan:
        run_overnight_loop("scan")
    elif args.both:
        run_overnight_loop("both")
    else:
        run_overnight_loop("research")
