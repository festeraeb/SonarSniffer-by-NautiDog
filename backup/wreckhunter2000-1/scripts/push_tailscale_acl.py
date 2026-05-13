#!/usr/bin/env python3
"""
push_tailscale_acl.py
─────────────────────
Push the ACL policy in scripts/tailscale_acl.hujson to Tailscale via API.

Usage:
    python scripts/push_tailscale_acl.py            # dry-run (print what would be sent)
    python scripts/push_tailscale_acl.py --apply    # actually push

Requirements:
    pip install requests
    TAILSCALE_API_KEY in .env  (or set as env var)
"""

import argparse
import json
import os
import re
import sys
from pathlib import Path

# ── Load .env ─────────────────────────────────────────────────────────────────
def _load_env(path: Path) -> dict:
    out = {}
    if path.exists():
        for line in path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, _, v = line.partition("=")
                out[k.strip()] = v.strip()
    return out

_dotenv = _load_env(Path(__file__).parent.parent / ".env")

TAILSCALE_API_KEY = os.environ.get("TAILSCALE_API_KEY",
                    _dotenv.get("TAILSCALE_API_KEY", ""))

# The tailnet can be "-" to use the tailnet that owns the API key
TAILNET = os.environ.get("TAILSCALE_TAILNET", _dotenv.get("TAILSCALE_TAILNET", "-"))

ACL_FILE = Path(__file__).parent / "tailscale_acl.hujson"
API_URL  = f"https://api.tailscale.com/api/v2/tailnet/{TAILNET}/acl"


def strip_comments(text: str) -> str:
    """Remove // … comments so the HuJSON can be parsed as standard JSON."""
    # Remove single-line comments
    text = re.sub(r'//[^\n]*', '', text)
    # Remove trailing commas before } or ]
    text = re.sub(r',(\s*[}\]])', r'\1', text)
    return text


def load_acl() -> dict:
    raw = ACL_FILE.read_text(encoding="utf-8")
    return json.loads(strip_comments(raw))


def push_acl(policy: dict, dry_run: bool = True) -> None:
    import requests

    if not TAILSCALE_API_KEY:
        print("ERROR: TAILSCALE_API_KEY not set in .env or environment.")
        sys.exit(1)

    headers = {
        "Authorization": f"Bearer {TAILSCALE_API_KEY}",
        "Content-Type":  "application/json",
    }

    # Tailscale API wants the policy wrapped in an "acls" / raw HuJSON body
    # but also accepts a JSON body when Content-Type is application/json.
    body = json.dumps(policy, indent=2)

    if dry_run:
        print("=== DRY RUN — would POST to:", API_URL)
        print("=== Headers:", {k: (v if k != "Authorization" else "Bearer ***") for k, v in headers.items()})
        print("=== Body:")
        print(body)
        print("\nRun with --apply to actually push.")
        return

    print(f"Pushing ACL to {API_URL} ...")
    resp = requests.post(API_URL, headers=headers, data=body, timeout=15)

    if resp.status_code in (200, 201):
        print(f"✓  ACL pushed successfully (HTTP {resp.status_code})")
        try:
            print(json.dumps(resp.json(), indent=2))
        except Exception:
            print(resp.text)
    else:
        print(f"✗  Failed: HTTP {resp.status_code}")
        print(resp.text)
        sys.exit(1)


def main():
    parser = argparse.ArgumentParser(description="Push Tailscale ACL policy via API")
    parser.add_argument("--apply", action="store_true",
                        help="Actually push (default is dry-run)")
    args = parser.parse_args()

    policy = load_acl()
    print(f"Loaded ACL from {ACL_FILE}")
    push_acl(policy, dry_run=not args.apply)


if __name__ == "__main__":
    main()
