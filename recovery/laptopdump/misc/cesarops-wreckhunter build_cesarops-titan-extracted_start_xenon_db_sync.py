#!/usr/bin/env python3
"""
Quick Xenon Database Sync Starter

Before nap: Run this to sync DB to Xenon and start it processing
"""

import subprocess
import sys
from pathlib import Path
from datetime import datetime

# Xenon config
XENON_USER = "cesarops"
XENON_HOST = "10.0.0.55"
XENON_PATH = "~/cesarops-wreckhunter-build"

# Database files to sync
DB_FILES = [
    "init_database.py",
    "cesarops_comprehensive_schema.sql",
    "database_connector.py",
    "triple_lock_fusion.py",
    "lake_michigan_scan.py",
]

def test_connection():
    """Test SSH connection to Xenon"""
    print("[1/4] Testing Xenon connection...")
    try:
        result = subprocess.run(
            f"ssh {XENON_USER}@{XENON_HOST} \"echo Connected\"",
            shell=True,
            capture_output=True,
            text=True,
            timeout=10
        )
        if result.returncode == 0:
            print(f"  ✓ Connected to Xenon ({XENON_HOST})")
            return True
        else:
            print(f"  ✗ Connection failed: {result.stderr.strip()}")
            return False
    except Exception as e:
        print(f"  ✗ Error: {e}")
        return False

def sync_files():
    """Sync database files to Xenon"""
    print("\n[2/4] Syncing database files to Xenon...")
    
    base = Path(__file__).parent
    synced = 0
    failed = 0
    
    for file in DB_FILES:
        src = base / file
        if not src.exists():
            print(f"  ⚠ Skipping {file} (not found)")
            continue
        
        # Use scp to copy
        cmd = f'scp "{src}" {XENON_USER}@{XENON_HOST}:{XENON_PATH}/'
        result = subprocess.run(cmd, shell=True, capture_output=True)
        
        if result.returncode == 0:
            print(f"  ✓ {file}")
            synced += 1
        else:
            print(f"  ✗ {file}")
            failed += 1
    
    print(f"\n  Synced: {synced} files, {failed} failed")
    return synced > 0

def start_database_on_xenon():
    """Start database initialization on Xenon"""
    print("\n[3/4] Starting database on Xenon...")
    
    commands = [
        f"cd {XENON_PATH}",
        "python3 init_database.py",
    ]
    
    cmd = f"ssh {XENON_USER}@{XENON_HOST} \"" + "; ".join(commands) + "\""
    print(f"  Running: {cmd}")
    print("  (This will run in background, check with: ssh cesarops@10.0.0.55)")
    
    # Start detached (don't wait for completion)
    subprocess.Popen(cmd, shell=True)
    print("  ✓ Database init started on Xenon")
    
    return True

def show_status():
    """Show final status"""
    print("\n[4/4] Status")
    print("="*70)
    print(f"  Xenon Host: {XENON_HOST}")
    print(f"  User: {XENON_USER}")
    print(f"  Path: {XENON_PATH}")
    print(f"  Time: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print("="*70)
    print("\n✓ Setup complete! Xenon is initializing the database.")
    print("\nWhen you wake up:")
    print(f"  ssh {XENON_USER}@{XENON_HOST}")
    print("  cd cesarops-wreckhunter-build")
    print("  python3 triple_lock_fusion.py")
    print("="*70)

def main():
    print("="*70)
    print("XENON DATABASE SYNC - PRE-NAP SETUP")
    print("="*70)
    print()
    
    # Test connection
    if not test_connection():
        print("\n✗ Cannot connect to Xenon. Check:")
        print("  1. Xenon is powered on")
        print("  2. Network connection (10.0.0.55)")
        print("  3. SSH credentials")
        sys.exit(1)
    
    # Sync files
    if not sync_files():
        print("\n✗ File sync failed")
        sys.exit(1)
    
    # Start database
    start_database_on_xenon()
    
    # Show status
    show_status()

if __name__ == "__main__":
    main()
