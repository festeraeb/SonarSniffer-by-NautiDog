#!/usr/bin/env python3
"""
Process TIFFs through M2200 and output coordinates
"""

import subprocess
import json
from pathlib import Path
import re

def process_tiff_with_coordinates(tiff_path: Path, threshold: float = 2.5):
    """Process TIFF and extract anomaly coordinates"""
    exe = Path("target/release/cesarops-gpu.exe")
    
    if not exe.exists():
        print(f"ERROR: {exe} not found. Run: cargo build --release --bin cesarops-gpu")
        return None
    
    print(f"Processing: {tiff_path.name}")
    
    result = subprocess.run(
        [str(exe), str(tiff_path), "--threshold", str(threshold)],
        capture_output=True,
        text=True
    )
    
    if result.returncode != 0:
        print(f"  ERROR: {result.stderr}")
        return None
    
    # Parse output for dimensions and anomalies
    width, height = None, None
    anomalies = []
    
    for line in result.stdout.split('\n'):
        # Extract dimensions
        if "Loaded" in line and "thermal data" in line:
            match = re.search(r'(\d+)x(\d+)', line)
            if match:
                width, height = int(match.group(1)), int(match.group(2))
        
        # Extract anomaly pixels
        if "Pixel" in line and "Z-Score" in line:
            match = re.search(r'Pixel \((\d+), (\d+)\): Z-Score ([\d.]+)', line)
            if match:
                row, col, zscore = int(match.group(1)), int(match.group(2)), float(match.group(3))
                
                # Convert pixel to approximate UTM (Lake Michigan UTM Zone 16T)
                # Assuming 30m/pixel resolution (Landsat/HLS)
                utm_easting = 450000 + (col * 30)  # Approximate
                utm_northing = 4700000 + ((height - row) * 30) if height else 0
                
                # Rough UTM to WGS84 (simplified - use proj for production)
                lat = utm_northing / 111320.0
                lon = -87.5 + (utm_easting - 500000) / 111320.0
                
                anomalies.append({
                    "pixel": {"row": row, "col": col},
                    "utm_16t": {"easting": utm_easting, "northing": utm_northing},
                    "wgs84": {"lat": lat, "lon": lon},
                    "zscore": zscore
                })
    
    return {
        "tiff": str(tiff_path),
        "dimensions": {"width": width, "height": height},
        "anomalies": anomalies,
        "gpu": "Quadro M2200" if "Quadro M2200" in result.stdout else "Unknown"
    }

def main():
    print("=" * 80)
    print("M2200 TIFF PROCESSOR - COORDINATE OUTPUT")
    print("=" * 80)
    print()
    
    # Test with synthetic TIFF
    test_tiff = Path("test_thermal.tif")
    
    if not test_tiff.exists():
        print("Creating test TIFF...")
        subprocess.run(["python", "test_m2200.py"], check=True)
    
    # Process TIFF
    result = process_tiff_with_coordinates(test_tiff, threshold=2.0)
    
    if result:
        # Save to JSON
        output_file = Path("coordinates_output.json")
        with open(output_file, 'w') as f:
            json.dump(result, f, indent=2)
        
        print(f"\nResults saved to: {output_file}")
        print(f"\nGPU Used: {result['gpu']}")
        print(f"Dimensions: {result['dimensions']['width']}x{result['dimensions']['height']}")
        print(f"Anomalies Found: {len(result['anomalies'])}")
        
        if result['anomalies']:
            print("\nTop 5 Anomalies with Coordinates:")
            for i, anom in enumerate(result['anomalies'][:5], 1):
                print(f"\n  [{i}] Pixel ({anom['pixel']['row']}, {anom['pixel']['col']})")
                print(f"      UTM-16T: E={anom['utm_16t']['easting']:.1f}m, N={anom['utm_16t']['northing']:.1f}m")
                print(f"      WGS84: {anom['wgs84']['lat']:.6f}°N, {anom['wgs84']['lon']:.6f}°W")
                print(f"      Z-Score: {anom['zscore']:.3f}")
        
        print("\n" + "=" * 80)
        print("COORDINATE OUTPUT COMPLETE")
        print("=" * 80)
        print(f"\nView full results: {output_file}")
        
        return True
    else:
        print("\nERROR: Processing failed")
        return False

if __name__ == "__main__":
    import sys
    success = main()
    sys.exit(0 if success else 1)
