#!/usr/bin/env python3
"""Build inbox/gemini_proposal.json from scrapes + pinned Google URL context."""
from __future__ import annotations

import json
from pathlib import Path

REPO = Path(__file__).resolve().parents[2]
COLLAB = REPO / "var" / "forge_collab"
SCRAPE = COLLAB / "scrapes"
INBOX = COLLAB / "inbox"


def main() -> int:
    url = (COLLAB / "COLLAB_GOOGLE_URL.txt").read_text().splitlines()[0].strip()
    txt_path = SCRAPE / "google_straits_bag_survey.txt"
    md_path = SCRAPE / "mackinac_survey_context.md"
    scrape_note = ""
    if txt_path.is_file():
        scrape_note = txt_path.read_text()[:2000]
    md_note = md_path.read_text() if md_path.is_file() else ""

    proposal = {
        "round_id": "straits_bag_survey_google",
        "from": "gemini_via_scrape",
        "source_url": url,
        "hypothesis": (
            "BAG hydro survey IDs and side-scan target numbering (e.g. 450 xx.xxx) "
            "anchor GT for Straits wrecks; optical/temporal tune uses known_wrecks_straits.json "
            "with Cedarville/Burns calibration."
        ),
        "physics": {
            "package": "cesarops-satellite",
            "files": ["src/temporal.rs", "src/phase_corr.rs", "scripts/known_wrecks_straits.json"],
            "changes": [
                "Cross-reference NOAA BAG surveys (NCEI) for future bathy corroboration",
                "Primary tune: phase corr + LOO persistence; sat-run metrics >= 0.30",
            ],
        },
        "forge": {
            "lane": "A",
            "prompt_tightening": [
                "Use scraped context in nautivecs ingest, not chat paste",
                "references must cite NCEI / survey IDs not invented paths",
            ],
        },
        "success_metrics": {
            "persistence_peak_min": 0.3,
            "gps_m_max": {"Cedarville": 300, "Burns": 300},
        },
        "references": [
            url,
            "https://www.ncei.noaa.gov/products/nos-hydro-survey",
            "https://www.govinfo.gov/content/pkg/CZIC-gc87-m5-g3-1992/html/CZIC-gc87-m5-g3-1992.htm",
        ],
        "scrape_excerpt": scrape_note,
        "cursor_synthesis": md_note[:8000],
        "do_not_invent": ["knobs.pon.rs", "cesarops-inference", "numpy"],
    }
    INBOX.mkdir(parents=True, exist_ok=True)
    out = INBOX / "gemini_proposal.json"
    out.write_text(json.dumps(proposal, indent=2) + "\n")
    print(f"wrote {out}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
