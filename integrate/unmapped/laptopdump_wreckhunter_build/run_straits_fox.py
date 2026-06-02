#!/usr/bin/env python3
"""
Quick runner for Straits + Fox Island area data processing.
No complex tool downloads - just satellite data and GPU processing.
"""

import sys
import subprocess
from pathlib import Path

REPO = Path(__file__).resolve().parent
WH2K = REPO / 'wreckhunter2000'

def check_deps():
    """Check if required Python packages are installed."""
    missing = []
    for pkg in ['h5py', 'numpy', 'rasterio', 'requests']:
        try:
            __import__(pkg)
        except ImportError:
            missing.append(pkg)
    
    if missing:
        print(f"[!] Missing packages: {', '.join(missing)}")
        print(f"[!] Install with: pip install {' '.join(missing)}")
        return False
    return True

def run_script(script_name):
    """Run a Python script and return success status."""
    script_path = WH2K / script_name
    if not script_path.exists():
        print(f"[!] Script not found: {script_path}")
        return False
    
    print(f"\n{'='*60}")
    print(f"Running: {script_name}")
    print('='*60)
    
    result = subprocess.run([sys.executable, str(script_path)])
    return result.returncode == 0

def main():
    print("="*60)
    print("STRAITS + FOX ISLAND - QUICK DATA RUN")
    print("="*60)
    print()
    print("Area: Straits of Mackinac to South Fox Island")
    print("Sensors: VIIRS LST (thermal) + VIIRS DNB (nighttime)")
    print("Years: 2012-2013 (low water)")
    print()
    
    # Check dependencies
    print("[*] Checking dependencies...")
    if not check_deps():
        print("\n[!] Install missing packages first, then re-run.")
        input("Press Enter to exit...")
        return 1
    print("[+] All dependencies OK")
    
    # Step 1: Download data
    if not run_script('straits_south_fox_historical_pull.py'):
        print("\n[!] Data download failed")
        input("Press Enter to exit...")
        return 1
    
    # Step 2: Process with GPU engine
    if not run_script('straits_south_fox_engine_runner.py'):
        print("\n[!] Processing failed")
        input("Press Enter to exit...")
        return 1
    
    # Success
    output_dir = WH2K / 'outputs' / 'straits_south_fox_historical' / 'engine_results'
    print("\n" + "="*60)
    print("SUCCESS!")
    print("="*60)
    print(f"\nResults saved to:")
    print(f"  {output_dir}")
    print()
    print("Key files:")
    print("  - straits_engine_detections.kml (open in Google Earth)")
    print("  - straits_engine_master_report.json (full data)")
    print()
    input("Press Enter to exit...")
    return 0

if __name__ == '__main__':
    sys.exit(main())
