#!/usr/bin/env python3
"""
Sync CESAROPS database connector to xenon server
"""

import subprocess
import sys
from pathlib import Path

# Files to sync
FILES_TO_SYNC = [
    "database_connector.py",
    "cesarops_comprehensive_schema.sql",
    "init_database.py",
]

# Xenon config
XENON_USER = "cesarops"
XENON_HOST = "10.0.0.55"
XENON_PATH = "~/cesarops-wreckhunter-build"

def sync_to_xenon():
    """Sync database files to xenon server"""
    print("="*80)
    print("SYNCING DATABASE CONNECTOR TO XENON")
    print("="*80)
    
    for file in FILES_TO_SYNC:
        src = Path(__file__).parent / file
        
        if not src.exists():
            print(f"  ⚠ Skipping {file} (not found)")
            continue
        
        print(f"  Syncing {file}...")
        
        # Use scp to copy
        cmd = f'scp "{src}" {XENON_USER}@{XENON_HOST}:{XENON_PATH}/'
        print(f"    {cmd}")
        
        # Note: This will prompt for password
        # For automated sync, set up SSH keys
        result = subprocess.run(cmd, shell=True)
        
        if result.returncode == 0:
            print(f"    ✓ Success")
        else:
            print(f"    ✗ Failed (exit code {result.returncode})")
    
    print("\n" + "="*80)
    print("SYNC COMPLETE")
    print("="*80)
    print("\nOn xenon, run:")
    print(f"  ssh {XENON_USER}@{XENON_HOST}")
    print(f"  cd {XENON_PATH}")
    print("  python database_connector.py")
    print("="*80)

def main():
    print("\nNote: You'll be prompted for the xenon password (cesarops)")
    print("For passwordless sync, set up SSH keys:")
    print("  ssh-keygen -t ed25519")
    print(f"  ssh-copy-id {XENON_USER}@{XENON_HOST}")
    print()
    
    sync_to_xenon()

if __name__ == "__main__":
    main()
