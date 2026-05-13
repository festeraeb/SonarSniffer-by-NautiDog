#!/usr/bin/env python3
"""
CESAROPS Overnight Research Daemon
===================================
Runs on T440 with Qwen3.6-35B on dual P100s.
Queries arXiv + Semantic Scholar, summarizes papers via the local LLM,
proposes improvements to the detection pipeline.

Usage:
    python3 scripts/run_research_overnight.py

Writes findings to research_log/YYYY-MM-DD.json
"""

import json
import os
import sys
import time
import urllib.request
import urllib.parse
from datetime import datetime, timezone
from pathlib import Path

# ── Config ─────────────────────────────────────────────────────────────────────

LLM_URL = os.environ.get("LLM_URL", "http://127.0.0.1:5001/v1/chat/completions")
LOG_DIR = Path(os.environ.get("LOG_DIR", "/home/cesarops/wreckhunter2000-1/research_log"))
INTERVAL_MINUTES = int(os.environ.get("INTERVAL", "30"))

LOG_DIR.mkdir(parents=True, exist_ok=True)

# ── Research domains with tight queries ────────────────────────────────────────

DOMAINS = {
    "wreck_detection": {
        "arxiv": "ti:shipwreck+OR+ti:submerged+vessel+OR+(ti:SAR+AND+ti:wreck)+OR+(ti:magnetometer+AND+ti:anomaly+AND+ti:marine)",
        "s2": "shipwreck detection SAR magnetometer submerged vessel sonar",
    },
    "hydrocarbon": {
        "arxiv": "ti:oil+spill+AND+(ti:SWIR+OR+ti:spectral+OR+ti:SAR)+AND+ti:detection",
        "s2": "oil spill SWIR spectral SAR detection ocean satellite",
    },
    "turbidity": {
        "arxiv": "ti:turbidity+AND+(ti:correction+OR+ti:compensation)+AND+(ti:bathymetry+OR+ti:remote+sensing)",
        "s2": "turbidity correction bathymetry optical depth remote sensing",
    },
    "sar_maritime": {
        "arxiv": "ti:SAR+AND+(ti:ship+OR+ti:vessel+OR+ti:maritime)+AND+(ti:detection+OR+ti:classification)",
        "s2": "SAR ship detection vessel classification Sentinel-1 maritime",
    },
    "bathymetry": {
        "arxiv": "ti:bathymetry+AND+(ti:satellite+OR+ti:ICESat+OR+ti:SWOT)+AND+(ti:shallow+OR+ti:coastal)",
        "s2": "satellite derived bathymetry ICESat-2 shallow water coastal",
    },
    "curvelet_gpu": {
        "arxiv": "ti:curvelet+OR+(ti:wavelet+AND+ti:GPU)+OR+(ti:Vulkan+AND+ti:compute+AND+ti:geospatial)",
        "s2": "curvelet transform GPU compute geospatial detection",
    },
}

# ── API helpers ────────────────────────────────────────────────────────────────

def fetch_arxiv(query: str, max_results: int = 5) -> list:
    url = f"https://export.arxiv.org/api/query?search_query={query}&start=0&max_results={max_results}&sortBy=submittedDate&sortOrder=descending"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "CESAROPS-Research/1.0"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            xml = resp.read().decode("utf-8")
        return parse_arxiv(xml)
    except Exception as e:
        print(f"  arXiv error: {e}")
        return []


def fetch_semantic_scholar(query: str, max_results: int = 5) -> list:
    encoded = urllib.parse.quote(query)
    url = f"https://api.semanticscholar.org/graph/v1/paper/search?query={encoded}&limit={max_results}&fields=title,authors,year,abstract,citationCount,openAccessPdf"
    try:
        req = urllib.request.Request(url, headers={"User-Agent": "CESAROPS-Research/1.0"})
        with urllib.request.urlopen(req, timeout=15) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        papers = []
        for p in data.get("data", []):
            if not p.get("title"):
                continue
            source = ""
            if p.get("openAccessPdf", {}).get("url"):
                source = p["openAccessPdf"]["url"]
            elif p.get("paperId"):
                source = f"https://www.semanticscholar.org/paper/{p['paperId']}"
            if not source:
                continue
            papers.append({
                "title": p["title"],
                "authors": [a.get("name", "") for a in (p.get("authors") or [])[:3]],
                "year": p.get("year", 0),
                "abstract": (p.get("abstract") or "")[:500],
                "citations": p.get("citationCount", 0),
                "source_url": source,
                "origin": "semantic_scholar",
            })
        return papers
    except Exception as e:
        print(f"  S2 error: {e}")
        return []


def parse_arxiv(xml: str) -> list:
    papers = []
    for entry in xml.split("<entry>")[1:]:
        title = _xml_text(entry, "title", "").replace("\n", " ").strip()
        abstract = _xml_text(entry, "summary", "").strip()[:500]
        arxiv_id = _xml_text(entry, "id", "").strip()
        authors = [a.strip() for a in entry.split("<name>")[1:]]
        authors = [a.split("</name>")[0] for a in authors][:3]
        if not title or not arxiv_id:
            continue
        source = arxiv_id if "arxiv.org" in arxiv_id else f"https://arxiv.org/abs/{arxiv_id}"
        papers.append({
            "title": title,
            "authors": authors,
            "year": int(_xml_text(entry, "published", "2024")[:4]),
            "abstract": abstract,
            "citations": 0,
            "source_url": source,
            "origin": "arxiv",
        })
    return papers


def _xml_text(xml: str, tag: str, default: str = "") -> str:
    start = xml.find(f"<{tag}>")
    if start == -1:
        start = xml.find(f"<{tag} ")
        if start == -1:
            return default
        start = xml.find(">", start) + 1
    else:
        start += len(tag) + 2
    end = xml.find(f"</{tag}>", start)
    if end == -1:
        return default
    return xml[start:end]


# ── LLM synthesis ──────────────────────────────────────────────────────────────

def synthesize(papers: list, domain: str) -> str:
    """Ask Qwen3.6 to analyze papers and propose improvements."""
    if not papers:
        return ""

    abstracts = "\n---\n".join([
        f"Title: {p['title']}\nSource: {p['source_url']}\nAbstract: {p['abstract']}"
        for p in papers[:3]
    ])

    prompt = f"""You are analyzing real research papers about {domain} for the CESAROPS shipwreck detection system.

Based ONLY on the abstracts below (do not invent facts), suggest ONE specific testable improvement to our detection pipeline.

Our system uses:
- Curvelet transforms (f64, AVX-512 Xeon) for sub-surface anomaly detection
- wgpu/Vulkan compute shaders (f32) for parallel dipole scanning on Tesla P100
- NOAA buoy weather data to pick optimal satellite download days
- Sentinel-1 SAR + optical multispectral fusion

Papers:
{abstracts}

Respond with:
1. Key technique from the papers (2 sentences max)
2. How it could improve CESAROPS (2 sentences max)
3. Specific parameter or algorithm change to test (1 sentence)
"""

    payload = json.dumps({
        "model": "default",
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 300,
        "temperature": 0.3,
    }).encode("utf-8")

    try:
        req = urllib.request.Request(
            LLM_URL,
            data=payload,
            headers={"Content-Type": "application/json"},
        )
        with urllib.request.urlopen(req, timeout=120) as resp:
            data = json.loads(resp.read().decode("utf-8"))
        return data["choices"][0]["message"]["content"]
    except Exception as e:
        print(f"  LLM error: {e}")
        return ""


# ── Main loop ──────────────────────────────────────────────────────────────────

def run_cycle(domain_name: str, domain_config: dict) -> dict:
    """Run one research cycle for a domain."""
    print(f"\n{'='*60}")
    print(f"  Domain: {domain_name}")
    print(f"{'='*60}")

    # Fetch papers
    print("  Querying arXiv...")
    arxiv_papers = fetch_arxiv(domain_config["arxiv"], max_results=5)
    print(f"  → {len(arxiv_papers)} papers")

    print("  Querying Semantic Scholar...")
    time.sleep(1)  # rate limit
    s2_papers = fetch_semantic_scholar(domain_config["s2"], max_results=5)
    print(f"  → {len(s2_papers)} papers")

    all_papers = arxiv_papers + s2_papers

    # Filter out irrelevant (basic check — title must have at least one domain keyword)
    keywords = domain_name.replace("_", " ").split()
    relevant = [p for p in all_papers if any(
        kw.lower() in p["title"].lower() for kw in keywords
    )] or all_papers[:3]  # fallback to top 3 if filter is too strict

    print(f"  Relevant: {len(relevant)} papers")
    for p in relevant[:3]:
        print(f"    • {p['title'][:70]}...")

    # Synthesize with LLM
    print("  Synthesizing with Qwen3.6...")
    synthesis = synthesize(relevant, domain_name)
    if synthesis:
        print(f"  ✓ Got synthesis ({len(synthesis)} chars)")
    else:
        print("  ✗ No synthesis")

    return {
        "domain": domain_name,
        "timestamp": datetime.now(timezone.utc).isoformat(),
        "papers_found": len(all_papers),
        "papers_relevant": len(relevant),
        "papers": [{"title": p["title"], "source": p["source_url"], "year": p["year"]} for p in relevant[:5]],
        "synthesis": synthesis,
    }


def main():
    print("╔══════════════════════════════════════════════════════════╗")
    print("║  CESAROPS Overnight Research Daemon                      ║")
    print("║  Model: Qwen3.6-35B-A3B on dual P100                    ║")
    print(f"║  Interval: {INTERVAL_MINUTES} min | Log: {LOG_DIR}     ║")
    print("╚══════════════════════════════════════════════════════════╝")

    cycle = 0
    domains = list(DOMAINS.items())

    while True:
        # Rotate through domains — one per cycle
        domain_name, domain_config = domains[cycle % len(domains)]
        cycle += 1

        result = run_cycle(domain_name, domain_config)

        # Save to log
        today = datetime.now(timezone.utc).strftime("%Y-%m-%d")
        log_file = LOG_DIR / f"{today}.json"

        # Append to daily log
        existing = []
        if log_file.exists():
            try:
                existing = json.loads(log_file.read_text())
            except:
                existing = []
        existing.append(result)
        log_file.write_text(json.dumps(existing, indent=2))
        print(f"\n  Saved to {log_file}")

        print(f"\n  Sleeping {INTERVAL_MINUTES} minutes until next cycle...")
        print(f"  (Cycle {cycle}, next domain: {domains[cycle % len(domains)][0]})")
        time.sleep(INTERVAL_MINUTES * 60)


if __name__ == "__main__":
    try:
        main()
    except KeyboardInterrupt:
        print("\n\nResearch daemon stopped.")
