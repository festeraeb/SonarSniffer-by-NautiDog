#!/usr/bin/env python3
"""
CESAROPS Overnight Research Agent
==================================
Autonomous research loop that uses the loaded LLM (Qwen3.6-35B on P100s)
to search scientific literature, analyze our current detection pipeline,
and propose improvements.

Runs overnight while you sleep. Writes findings to research_log/.

What it does:
  1. Searches Crossref, Semantic Scholar, and arXiv for papers related to:
     - Curvelet transforms for sub-surface detection
     - Aeromagnetic dipole anomaly detection
     - Satellite-based shipwreck detection
     - Richardson number oceanographic layering
     - wgpu/Vulkan GPU compute for geospatial
     - MoE LLM inference optimization
  2. Summarizes relevant papers via the local LLM
  3. Compares findings against our current algorithms
  4. Writes improvement proposals with code snippets
  5. Logs everything to research_log/YYYY-MM-DD_session.json

Usage:
    python research_agent.py --run              # Start overnight loop
    python research_agent.py --once             # Single research pass
    python research_agent.py --topics           # List research topics
    python research_agent.py --report           # Show accumulated findings
    python research_agent.py --propose          # Generate improvement proposals from findings

Environment:
    KOBOLD_URL          — LLM endpoint (default: http://100.72.182.77:5001)
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

# Fix Windows console
if sys.platform == 'win32':
    try:
        sys.stdout.reconfigure(encoding='utf-8')
        sys.stderr.reconfigure(encoding='utf-8')
    except Exception:
        pass

REPO = Path(__file__).resolve().parent
LOG_DIR = REPO / "research_log"
LOG_DIR.mkdir(parents=True, exist_ok=True)

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

KOBOLD_URL = _cfg("KOBOLD_URL", "https://llm.cesarops.org")
RESEARCH_INTERVAL = int(_cfg("RESEARCH_INTERVAL", "30"))
MAX_PAPERS = int(_cfg("RESEARCH_MAX_PAPERS", "5"))

