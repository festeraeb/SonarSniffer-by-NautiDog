#!/usr/bin/env python3
"""Backward-compatible wrapper for production_satellite_downloader.py."""

import os
import subprocess
import sys
from pathlib import Path


def main() -> int:
    runner = Path("/codebase/repos/wreckhunter2000-1/scripts/production_satellite_downloader.py")
    cmd = ["python3", str(runner), *sys.argv[1:]]
    proc = subprocess.run(cmd, env=dict(os.environ), text=True)
    return proc.returncode


if __name__ == "__main__":
    raise SystemExit(main())

