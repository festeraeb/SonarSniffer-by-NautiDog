#!/usr/bin/env python3
"""
CESAROPS CLI - Python orchestrator for Rust GPU engine
Handles TIFF discovery and hands off to Rust for GPU processing
"""

import subprocess
import sys
from pathlib import Path
import json

from wreckhunter2000.scripts.tools.gpu_batch_runner import run_multi_profile_audit
from wreckhunter2000.scripts.tools.buoy_weather_checker import select_glint_windows


def find_tiff_files(data_dir: Path) -> list[Path]:
    """Find all thermal TIFF files"""
    patterns = ["**/*B10.tif", "**/*B11.tif"]
    tiffs = []
    for pattern in patterns:
        tiffs.extend(data_dir.glob(pattern))
    return sorted(set(tiffs))

def run_rust_gpu_engine(tiff_path: Path, output_dir: Path) -> dict:
    """Execute Rust GPU engine on single TIFF"""
    rust_exe = Path(__file__).parent / "target" / "release" / "cesarops-gpu.exe"
    
    if not rust_exe.exists():
        print(f"ERROR: Rust engine not built. Run: cargo build --release --bin cesarops-gpu")
        sys.exit(1)
    
    print(f"  → Processing {tiff_path.name} with Rust GPU engine...")
    
    result = subprocess.run(
        [str(rust_exe), str(tiff_path)],
        capture_output=True,
        text=True
    )
    
    if result.returncode != 0:
        print(f"  ✗ GPU engine failed: {result.stderr}")
        return {"status": "failed", "error": result.stderr}
    
    print(f"  ✓ GPU processing complete")
    return {"status": "success", "stdout": result.stdout}

def main():
    print("=" * 80)
    print("CESAROPS CLI - Python → Rust GPU Pipeline")
    print("=" * 80)
    print()
    
    # Configuration
    data_dir = Path(r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw")
    output_dir = Path(__file__).parent / "outputs"
    output_dir.mkdir(exist_ok=True)
    
    # Find TIFFs
    print(f"[1/3] Scanning {data_dir} for thermal TIFFs...")
    tiffs = find_tiff_files(data_dir)
    print(f"  Found {len(tiffs)} thermal TIFF files")
    
    if not tiffs:
        print("  No TIFFs found. Run fetcher.py first.")
        sys.exit(1)

    # Environmental pre-screen using buoy weather
    print(f"\n[1.5/3] Pre-screening using NDBC buoy data (glint windows)...")
    glint_days = select_glint_windows('45002', 2025, max_days=8)
    print(f"  Selected {len(glint_days)} glint days: {glint_days}")

    # Process each TIFF with Rust GPU engine and the new multi-profile GPU audit
    print(f"\n[2/3] Processing TIFFs with Rust GPU engine and multi-profile audit...")
    results = []
    
    for i, tiff in enumerate(tiffs[:5], 1):  # Limit to 5 for testing
        print(f"\n  [{i}/{min(5, len(tiffs))}] {tiff.name}")
        result = run_rust_gpu_engine(tiff, output_dir)
        results.append({"tiff": str(tiff), "result": result})

        # Run our dual-profile GPU audit from tools folder
        audit_results = run_multi_profile_audit(str(tiff), cooldown_sec=3)
        debug_file = output_dir / f"audit_many_{tiff.stem}.json"
        with open(debug_file, "w") as df:
            json.dump(audit_results, df, indent=2)
        print(f"  ✓ Multi-profile audit found {len(audit_results)} hits, saved to {debug_file}")
    
    # Save results
    print(f"\n[3/3] Saving results...")
    results_file = output_dir / "gpu_scan_results.json"
    with open(results_file, "w") as f:
        json.dump(results, f, indent=2)
    
    print(f"  ✓ Results saved to {results_file}")
    print()
    print("=" * 80)
    print("SCAN COMPLETE")
    print("=" * 80)

if __name__ == "__main__":
    main()
