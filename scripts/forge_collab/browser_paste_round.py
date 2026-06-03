#!/usr/bin/env python3
"""Paste collab payload into open Google AI textarea via Playwright (headed optional)."""
import json, sys
from pathlib import Path
try:
    from playwright.sync_api import sync_playwright
except ImportError:
    sys.exit("pip install playwright && playwright install chromium")

REPO = Path(__file__).resolve().parents[2]
payload = Path(sys.argv[1] if len(sys.argv) > 1 else REPO / "var/forge_collab/outbox/browser_round2_payload.txt").read_text()
url = (REPO / "var/forge_collab/COLLAB_GOOGLE_URL.txt").read_text().splitlines()[0].strip()

with sync_playwright() as p:
    browser = p.chromium.launch(headless=False, channel="chrome") if "--chrome" in sys.argv else p.chromium.launch(headless=False)
    page = browser.new_page()
    page.goto(url, wait_until="domcontentloaded", timeout=120000)
    page.wait_for_timeout(3000)
    ta = page.locator("textarea").filter(has=page.locator("[placeholder*=Ask]")).first
    if ta.count() == 0:
        ta = page.locator("textarea").first
    ta.fill(payload)
    page.locator("button[aria-label=Send]").click()
    page.wait_for_timeout(25000)
    text = page.inner_text("body")
    out = REPO / "var/forge_collab/scrapes/google_round_latest.txt"
    out.write_text(text[-20000:])
    print("wrote", out, "chars", len(text))
    browser.close()
