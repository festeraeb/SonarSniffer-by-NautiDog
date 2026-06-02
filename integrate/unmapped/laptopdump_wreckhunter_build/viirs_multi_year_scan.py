#!/usr/bin/env python3
"""
VIIRS MULTI-YEAR FORENSIC SCAN - GPU ACCELERATED
Processes VNP21A1D (thermal) + VNP46A1 (optical/blue) data
Cross-references detections across multiple years
Color-codes by confirmation level

FORCES GPU1 (Quadro M2200) - See GPU_WORKFLOW.md
"""

import json
import numpy as np
from pathlib import Path
from datetime import datetime
from typing import Dict, List, Tuple

# FORCE GPU1 (Quadro M2200) - See GPU_SCRIPTS_SUMMARY.md
try:
    import cupy as cp
    # Force GPU1 (Quadro M2200) - matches cesarops-gpu.exe pattern
    cp.cuda.Device(1).use()
    print(f"✓ GPU1 Selected: {cp.cuda.Device(1).name}")
    print(f"  VRAM: {cp.cuda.Device(1).memory / 1e9:.2f} GB")
    HAS_CUPY = True
except Exception as e:
    print(f"⚠ GPU1 selection failed: {e}")
    print("  Falling back to CPU")
    HAS_CUPY = False

import rasterio
from rasterio.warp import transform as warp_transform

# Full path to VIIRS data
VIIRS_DATA_DIR = Path(r"c:\Users\thomf\programming\cesarops-wreckhunter build\wreckhunter2000\outputs\straits_south_fox_historical\engine_results")

# Output directory
OUTPUT_DIR = Path(r"c:\Users\thomf\programming\cesarops-wreckhunter build\wreckhunter2000\outputs\viirs_multi_year_fusion")
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Z-score tolerance for "same target" confirmation
ZSCORE_TOLERANCE = 0.5  # Within 0.5 = same target
LOCATION_TOLERANCE_M = 100  # Within 100m = same location


def parse_viirs_filename(filename: str) -> Dict:
    """Parse VIIRS filename to extract date and product info"""
    # Example: VNP21A1D.A2012092.h12v04.002.2024104024651_clip.tif
    parts = filename.split('.')
    if len(parts) < 2:
        return None
    
    product = parts[0]  # VNP21A1D or VNP46A1
    date_str = parts[1]  # A2012092
    
    if date_str.startswith('A'):
        year = int(date_str[1:5])
        doy = int(date_str[5:8])  # Day of year
        date = datetime.strptime(f"{year}-{doy}", "%Y-%j")
    else:
        date = None
    
    return {
        'product': product,
        'date': date,
        'year': year if date else None,
        'filename': filename,
    }


def process_viirs_tile(tiff_path: Path) -> List[Dict]:
    """Process single VIIRS tile and extract anomalies - GPU ACCELERATED"""
    detections = []
    
    try:
        with rasterio.open(tiff_path) as src:
            data = src.read(1).astype(np.float32)
            
            # Mask nodata
            nodata_mask = data == -9999
            
            if HAS_CUPY:
                # GPU processing
                data_gpu = cp.asarray(data)
                data_gpu[nodata_mask] = cp.nan
                
                # Calculate stats on GPU
                mean = float(cp.nanmean(data_gpu))
                std = float(cp.nanstd(data_gpu))
                
                if std == 0:
                    cp.get_default_memory_pool().free_all_blocks()
                    return []
                
                # Z-scores on GPU
                zscore_gpu = (data_gpu - mean) / std
                
                # Find anomalies
                threshold = 2.5
                anomaly_mask_gpu = cp.abs(zscore_gpu) > threshold
                
                if cp.sum(anomaly_mask_gpu) == 0:
                    cp.get_default_memory_pool().free_all_blocks()
                    return []
                
                # Get coordinates
                coords = cp.where(anomaly_mask_gpu)
                rows = coords[0].get()
                cols = coords[1].get()
                zscores = zscore_gpu[anomaly_mask_gpu].get()
                data_vals = data_gpu[anomaly_mask_gpu].get()
                
                cp.get_default_memory_pool().free_all_blocks()
                
                for row, col, zscore, val in zip(rows, cols, zscores, data_vals):
                    lon, lat = src.xy(row, col)
                    detections.append({
                        'lat': lat,
                        'lon': lon,
                        'zscore': float(zscore),
                        'abs_zscore': abs(float(zscore)),
                        'row': int(row),
                        'col': int(col),
                        'source_file': tiff_path.name,
                        'pixel_value': float(val),
                    })
            else:
                # CPU fallback
                data[nodata_mask] = np.nan
                
                mean = float(np.nanmean(data))
                std = float(np.nanstd(data))
                
                if std == 0:
                    return []
                
                zscore = (data - mean) / std
                threshold = 2.5
                anomaly_mask = np.abs(zscore) > threshold
                
                if np.sum(anomaly_mask) == 0:
                    return []
                
                coords = np.where(anomaly_mask)
                
                for row, col in zip(coords[0], coords[1]):
                    lon, lat = src.xy(row, col)
                    detections.append({
                        'lat': lat,
                        'lon': lon,
                        'zscore': float(zscore[row, col]),
                        'abs_zscore': abs(float(zscore[row, col])),
                        'row': int(row),
                        'col': int(col),
                        'source_file': tiff_path.name,
                        'pixel_value': float(data[row, col]),
                    })
            
            return detections
            
    except Exception as e:
        print(f"  Error processing {tiff_path.name}: {e}")
        return []


def cluster_detections(all_detections: List[Dict]) -> List[Dict]:
    """
    Cluster detections by location and Z-score similarity.
    Multiple detections at same location = confirmed target.
    """
    if not all_detections:
        return []
    
    clusters = []
    used = [False] * len(all_detections)
    
    for i, det in enumerate(all_detections):
        if used[i]:
            continue
        
        # Start new cluster
        cluster = [det]
        used[i] = True
        
        # Find nearby detections with similar Z-score
        for j, other in enumerate(all_detections):
            if used[j]:
                continue
            
            # Calculate distance (simplified - using lat/lon degrees)
            dlat = det['lat'] - other['lat']
            dlon = det['lon'] - other['lon']
            dist_m = np.sqrt(dlat**2 + dlon**2) * 111320  # Convert to meters
            
            # Check Z-score similarity
            zscore_diff = abs(det['abs_zscore'] - other['abs_zscore'])
            
            if dist_m < LOCATION_TOLERANCE_M and zscore_diff < ZSCORE_TOLERANCE:
                cluster.append(other)
                used[j] = True
        
        clusters.append(cluster)
    
    # Convert clusters to confirmed targets
    confirmed_targets = []
    
    for cluster in clusters:
        # Sort by date
        cluster.sort(key=lambda x: x.get('date', datetime.min))
        
        # Determine confirmation level
        num_dates = len(set([str(d.get('date', '')) for d in cluster]))
        avg_zscore = np.mean([d['abs_zscore'] for d in cluster])
        zscore_std = np.std([d['abs_zscore'] for d in cluster])
        
        # Color based on confirmation
        if num_dates >= 4 and zscore_std < 0.5:
            confidence = 'MULTI_YEAR_LOCK'
            color = 'blue'  # 🔵
        elif num_dates >= 3:
            confidence = 'TRIPLE_CONFIRMED'
            color = 'green'  # 🟢
        elif num_dates >= 2:
            confidence = 'DOUBLE_CONFIRMED'
            color = 'yellow'  # 🟡
        else:
            confidence = 'SINGLE_DETECTION'
            color = 'red'  # 🔴
        
        # Get representative location (centroid)
        avg_lat = np.mean([d['lat'] for d in cluster])
        avg_lon = np.mean([d['lon'] for d in cluster])
        
        confirmed_targets.append({
            'lat': float(avg_lat),
            'lon': float(avg_lon),
            'avg_zscore': float(avg_zscore),
            'zscore_std': float(zscore_std),
            'num_detections': len(cluster),
            'num_dates': num_dates,
            'confidence': confidence,
            'color': color,
            'dates': [str(d.get('date', 'Unknown')) for d in cluster],
            'source_files': list(set([d['source_file'] for d in cluster])),
            'all_detections': cluster,
        })
    
    # Sort by confidence (best first)
    confidence_order = {'MULTI_YEAR_LOCK': 0, 'TRIPLE_CONFIRMED': 1, 'DOUBLE_CONFIRMED': 2, 'SINGLE_DETECTION': 3}
    confirmed_targets.sort(key=lambda x: (confidence_order.get(x['confidence'], 99), -x['avg_zscore']))
    
    return confirmed_targets


def create_kmz(confirmed_targets: List[Dict], output_path: Path):
    """Create KMZ with color-coded pins by confirmation level"""
    try:
        import simplekml
    except ImportError:
        print("simplekml not available, skipping KMZ")
        return
    
    kml = simplekml.Kml()
    
    # Create folders by confidence level
    folders = {
        'blue': kml.newfolder(name="🔵 MULTI-YEAR LOCK (4+ dates, Z±0.5)"),
        'green': kml.newfolder(name="🟢 TRIPLE CONFIRMED (3+ dates)"),
        'yellow': kml.newfolder(name="🟡 DOUBLE CONFIRMED (2 dates)"),
        'red': kml.newfolder(name="🔴 SINGLE DETECTION (1 date)"),
    }
    
    for target in confirmed_targets:
        folder = folders.get(target['color'], folders['red'])
        
        # Icon style
        icon = simplekml.IconStyle(
            icon=simplekml.Icon(href=f"http://maps.google.com/mapfiles/kml/paddle/{target['color']}-circle.png"),
            scale=1.2
        )
        
        pnt = folder.newpoint(
            name=f"Z={target['avg_zscore']:.1f} ({target['confidence']})",
            coords=[(target['lon'], target['lat'])]
        )
        pnt.style.iconstyle = icon
        
        # Description
        dates_str = '<br/>'.join(target['dates'][:5])
        if len(target['dates']) > 5:
            dates_str += f"<br/>... and {len(target['dates']) - 5} more"
        
        pnt.description = f"""
        <![CDATA[
        <h3>{target['confidence']}</h3>
        <table>
            <tr><td><b>Avg Z-Score:</b></td><td>{target['avg_zscore']:.2f} ± {target['zscore_std']:.2f}</td></tr>
            <tr><td><b>Detections:</b></td><td>{target['num_detections']}</td></tr>
            <tr><td><b>Dates:</b></td><td>{target['num_dates']}</td></tr>
            <tr><td><b>Latitude:</b></td><td>{target['lat']:.6f}</td></tr>
            <tr><td><b>Longitude:</b></td><td>{target['lon']:.6f}</td></tr>
        </table>
        <br/>
        <b>Detection Dates:</b><br/>
        {dates_str}
        <br/><br/>
        <i>Multi-year confirmation = High confidence target</i>
        ]]>
        """
    
    kml.save(str(output_path))
    print(f"[OK] KMZ saved: {output_path}")


def main():
    print("="*120)
    print("VIIRS MULTI-YEAR FORENSIC SCAN - STRAITS OF MACKINAC / FOX ISLANDS")
    print("="*120)
    print()
    
    # Find all VIIRS TIFF files
    print("[STEP 1/4] FINDING VIIRS TILES...")
    tiff_files = list(VIIRS_DATA_DIR.glob("*.tif"))
    print(f"  Found {len(tiff_files)} TIFF files")
    
    # Parse filenames and organize by product/date
    tiles_by_product = {}
    for tiff in tiff_files:
        info = parse_viirs_filename(tiff.name)
        if info is None:
            continue
        
        product = info['product']
        if product not in tiles_by_product:
            tiles_by_product[product] = []
        
        info['path'] = tiff
        tiles_by_product[product].append(info)
    
    print(f"  VNP21A1D (Thermal): {len(tiles_by_product.get('VNP21A1D', []))} tiles")
    print(f"  VNP46A1 (Optical): {len(tiles_by_product.get('VNP46A1', []))} tiles")
    print()
    
    # Process each tile
    print("[STEP 2/4] PROCESSING TILES...")
    all_detections = []
    
    for product, tiles in tiles_by_product.items():
        print(f"\n  Processing {product}...")
        for tile in sorted(tiles, key=lambda x: str(x.get('date', ''))):
            detections = process_viirs_tile(tile['path'])
            
            for det in detections:
                det['product'] = product
                det['date'] = tile['date']
                det['year'] = tile['year']
            
            all_detections.extend(detections)
            print(f"    {tile['path'].name}: {len(detections)} anomalies")
    
    print(f"\n  Total raw detections: {len(all_detections)}")
    print()
    
    # Cluster and confirm
    print("[STEP 3/4] CLUSTERING & MULTI-YEAR CONFIRMATION...")
    confirmed = cluster_detections(all_detections)
    
    print(f"  Confirmed targets: {len(confirmed)}")
    
    # Count by confidence
    confidence_counts = {}
    for target in confirmed:
        conf = target['confidence']
        confidence_counts[conf] = confidence_counts.get(conf, 0) + 1
    
    for conf, count in sorted(confidence_counts.items()):
        print(f"    {conf}: {count} targets")
    print()
    
    # Save results
    print("[STEP 4/4] SAVING RESULTS...")
    
    # JSON
    json_path = OUTPUT_DIR / "viirs_multi_year_targets.json"
    with open(json_path, 'w') as f:
        json.dump({
            'scan_date': datetime.now().isoformat(),
            'data_source': str(VIIRS_DATA_DIR),
            'total_tiles': len(tiff_files),
            'total_detections': len(all_detections),
            'confirmed_targets': len(confirmed),
            'confidence_breakdown': confidence_counts,
            'targets': confirmed,
        }, f, indent=2)
    print(f"  JSON saved: {json_path}")
    
    # KMZ
    kmz_path = OUTPUT_DIR / "viirs_multi_year_targets.kmz"
    create_kmz(confirmed, kmz_path)
    
    # Summary
    print()
    print("="*120)
    print("SCAN COMPLETE")
    print("="*120)
    print(f"  Total tiles processed: {len(tiff_files)}")
    print(f"  Raw detections: {len(all_detections)}")
    print(f"  Confirmed targets: {len(confirmed)}")
    print()
    print("  COLOR LEGEND:")
    print("    🔵 BLUE   = Multi-year lock (4+ dates, Z-score within ±0.5)")
    print("    🟢 GREEN  = Triple confirmed (3+ dates)")
    print("    🟡 YELLOW = Double confirmed (2 dates)")
    print("    🔴 RED    = Single detection (1 date)")
    print()
    print(f"  Open in Google Earth: {kmz_path}")
    print("="*120)


if __name__ == '__main__':
    main()
