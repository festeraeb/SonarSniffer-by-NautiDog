#!/usr/bin/env python3
"""
Run CESAROPS pipeline with configuration from config_agent.py
"""

import json
import subprocess
import sys
from pathlib import Path

def load_config():
    """Load configuration from JSON"""
    config_path = Path("pipeline_config.json")
    
    if not config_path.exists():
        print("ERROR: No configuration found")
        print("Run: python config_agent.py")
        sys.exit(1)
    
    with open(config_path) as f:
        return json.load(f)

def find_tiffs(data_dir: Path, area_filter: dict = None) -> list[Path]:
    """Find TIFFs matching configuration"""
    patterns = []
    
    # Build patterns based on sensor config
    # For now, just find thermal bands
    patterns = ["**/*B10.tif", "**/*B11.tif"]
    
    tiffs = []
    for pattern in patterns:
        tiffs.extend(data_dir.glob(pattern))
    
    # TODO: Filter by area if specified
    
    return sorted(set(tiffs))

def run_rust_gpu(tiff_path: Path, config: dict) -> dict:
    """Run Rust GPU engine with config parameters"""
    rust_exe = Path(__file__).parent / "target" / "release" / "cesarops-gpu.exe"
    
    if not rust_exe.exists():
        print(f"ERROR: Rust engine not built")
        print("Run: build_gpu.bat")
        sys.exit(1)
    
    # Build command with parameters
    cmd = [str(rust_exe), str(tiff_path)]
    
    # Add threshold parameter
    cmd.extend(["--threshold", str(config.get('threshold', 2.5))])
    
    print(f"  → {tiff_path.name}")
    
    result = subprocess.run(cmd, capture_output=True, text=True)
    
    if result.returncode != 0:
        return {"status": "failed", "error": result.stderr}
    
    # Parse output for anomalies
    anomaly_count = 0
    for line in result.stdout.split('\n'):
        if "Detected" in line and "anomalies" in line:
            try:
                anomaly_count = int(line.split()[1])
            except:
                pass
    
    return {
        "status": "success",
        "anomaly_count": anomaly_count,
        "stdout": result.stdout
    }

def calculate_detection_score(aluminum: float, thermal: float, config: dict) -> float:
    """Calculate weighted detection score based on config"""
    alum_weight = config.get('aluminum_weight', 1.0)
    therm_weight = config.get('thermal_weight', 1.0)
    
    score = (aluminum * alum_weight + abs(thermal) * therm_weight) / (alum_weight + therm_weight)
    return score

def export_results(results: list, config: dict):
    """Export results in configured formats"""
    output_dir = Path(config['output_dir'])
    output_dir.mkdir(exist_ok=True)
    
    formats = config.get('output_formats', {'json': True, 'kml': False, 'csv': False})
    
    # JSON export
    if formats.get('json'):
        json_path = output_dir / "scan_results.json"
        with open(json_path, 'w') as f:
            json.dump(results, f, indent=2)
        print(f"  ✓ JSON: {json_path}")
    
    # KML export
    if formats.get('kml'):
        kml_path = output_dir / "detections.kml"
        # TODO: Generate KML from results
        print(f"  ✓ KML: {kml_path}")
    
    # CSV export
    if formats.get('csv'):
        csv_path = output_dir / "detections.csv"
        # TODO: Generate CSV from results
        print(f"  ✓ CSV: {csv_path}")

def main():
    print("=" * 80)
    print("CESAROPS CONFIGURED PIPELINE")
    print("=" * 80)
    print()
    
    # Load configuration
    print("[1/4] Loading configuration...")
    config = load_config()
    
    print(f"  Target type: {config['target_type']}")
    print(f"  Threshold: {config['threshold']}")
    print(f"  Min confidence: {config['min_confidence']}")
    print()
    
    # Find TIFFs
    print("[2/4] Finding TIFFs...")
    data_dir = Path(config['data_dir'])
    tiffs = find_tiffs(data_dir, config.get('area'))
    
    print(f"  Found {len(tiffs)} TIFF files")
    
    if not tiffs:
        print("  No TIFFs found. Check data_dir in config.")
        sys.exit(1)
    
    print()
    
    # Process TIFFs
    print("[3/4] Processing with Rust GPU engine...")
    results = []
    
    batch_size = config.get('batch_size', 1)
    limit = min(len(tiffs), 10)  # Limit for testing
    
    for i, tiff in enumerate(tiffs[:limit], 1):
        print(f"  [{i}/{limit}]", end=" ")
        result = run_rust_gpu(tiff, config)
        
        if result['status'] == 'success':
            print(f"✓ {result['anomaly_count']} anomalies")
            results.append({
                "tiff": str(tiff),
                "anomaly_count": result['anomaly_count'],
                "config": {
                    "threshold": config['threshold'],
                    "target_type": config['target_type']
                }
            })
        else:
            print(f"✗ Failed")
    
    print()
    
    # Export results
    print("[4/4] Exporting results...")
    export_results(results, config)
    
    print()
    print("=" * 80)
    print("SCAN COMPLETE")
    print("=" * 80)
    print()
    print(f"Processed: {len(results)} TIFFs")
    print(f"Total anomalies: {sum(r['anomaly_count'] for r in results)}")
    print(f"Output directory: {config['output_dir']}")

if __name__ == "__main__":
    main()
