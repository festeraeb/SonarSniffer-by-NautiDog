"""
Cross-reference PDF coordinates against BAG file
Test which locations show wreck signatures vs scrubbing
All measurements in FEET and NAUTICAL MILES
"""
import fitz
import re
import rasterio
import numpy as np
from scipy import ndimage
from pyproj import Transformer
from datetime import datetime

# Conversion constants
M_TO_FT = 3.28084
NM_TO_FT = 6076.12
FT_TO_NM = 1 / 6076.12

def extract_pdf_coordinates(pdf_path):
    """Extract all coordinates from PDF"""
    doc = fitz.open(pdf_path)
    coords = []
    
    for page_num, page in enumerate(doc):
        text = page.get_text()
        
        # DMS format
        lat_pattern = r"(\d{2})\s*[°]\s*(\d{2})\s*['\u2032]\s*(\d{2}\.?\d*)\s*[\"\u2033]?\s*([NS])"
        lon_pattern = r"(\d{2,3})\s*[°]\s*(\d{2})\s*['\u2032]\s*(\d{2}\.?\d*)\s*[\"\u2033]?\s*([EW])"
        
        lat_matches = re.findall(lat_pattern, text)
        lon_matches = re.findall(lon_pattern, text)
        
        for lat_m, lon_m in zip(lat_matches, lon_matches):
            lat = float(lat_m[0]) + float(lat_m[1])/60 + float(lat_m[2])/3600
            if lat_m[3] == 'S': lat = -lat
            
            lon = float(lon_m[0]) + float(lon_m[1])/60 + float(lon_m[2])/3600
            if lon_m[3] == 'W': lon = -lon
            
            coords.append({'lat': lat, 'lon': lon, 'page': page_num + 1})
    
    doc.close()
    
    # Deduplicate
    unique = []
    for c in coords:
        is_dup = any(abs(c['lat']-u['lat']) < 0.0001 and abs(c['lon']-u['lon']) < 0.0001 for u in unique)
        if not is_dup:
            unique.append(c)
    
    return unique

def analyze_bag_location(bag_src, lat, lon, transformer, search_radius_ft=300):
    """Analyze a location in BAG file for wreck signatures"""
    elevation = bag_src.read(1)
    transform = bag_src.transform
    pixel_size_m = abs(transform[0])
    pixel_size_ft = pixel_size_m * M_TO_FT
    
    # Convert lat/lon to pixel
    x, y = transformer.transform(lon, lat)
    col = int((x - transform[2]) / transform[0])
    row = int((y - transform[5]) / transform[4])
    
    # Check bounds
    h, w = elevation.shape
    if col < 0 or col >= w or row < 0 or row >= h:
        return None
    
    # Extract local area
    search_px = int(search_radius_ft / pixel_size_ft)
    r1, r2 = max(0, row-search_px), min(h, row+search_px)
    c1, c2 = max(0, col-search_px), min(w, col+search_px)
    
    local = elevation[r1:r2, c1:c2]
    valid_mask = (local > -1000) & (local < 1000)
    valid = local[valid_mask]
    
    if len(valid) < 100:
        return None
    
    # Calculate metrics in FEET
    depth_ft = np.abs(np.mean(valid)) * M_TO_FT
    std_ft = np.std(valid) * M_TO_FT
    range_ft = (np.max(valid) - np.min(valid)) * M_TO_FT
    
    # Gradient analysis
    local_clean = np.where(valid_mask, local, np.nan)
    gx = ndimage.sobel(local_clean, axis=1)
    gy = ndimage.sobel(local_clean, axis=0)
    gradient = np.sqrt(gx**2 + gy**2)
    grad_mean_ft = np.nanmean(gradient) * M_TO_FT
    grad_max_ft = np.nanmax(gradient) * M_TO_FT
    
    # Shape analysis - look for elongated depressions
    below_mean = local_clean < (np.nanmean(local_clean) - np.nanstd(local_clean))
    if np.any(below_mean):
        rows_anomaly = np.sum(np.any(below_mean, axis=1)) * pixel_size_ft
        cols_anomaly = np.sum(np.any(below_mean, axis=0)) * pixel_size_ft
        aspect = max(rows_anomaly, cols_anomaly) / max(min(rows_anomaly, cols_anomaly), 1)
    else:
        rows_anomaly, cols_anomaly, aspect = 0, 0, 1
    
    # Classification
    if std_ft < 1.0 and grad_mean_ft < 0.15:
        signature = "SCRUBBED"
    elif grad_max_ft > 3.0 and aspect > 1.8:
        signature = "WRECK_SIGNATURE"
    elif grad_mean_ft > 0.8:
        signature = "HIGH_TEXTURE"
    elif std_ft > 2.5:
        signature = "VARIED_TERRAIN"
    else:
        signature = "NORMAL"
    
    return {
        'depth_ft': round(depth_ft, 1),
        'std_ft': round(std_ft, 2),
        'range_ft': round(range_ft, 1),
        'grad_mean_ft': round(grad_mean_ft, 3),
        'grad_max_ft': round(grad_max_ft, 2),
        'extent_ft': (round(rows_anomaly, 0), round(cols_anomaly, 0)),
        'aspect_ratio': round(aspect, 1),
        'signature': signature
    }


def main():
    print("=" * 80)
    print("PDF-TO-BAG CROSS-REFERENCE ANALYSIS")
    print("All measurements in FEET and NAUTICAL MILES")
    print("=" * 80)
    
    # PDFs and their corresponding BAG files
    pdf_bag_pairs = [
        ("PDFS Original/DOC-NOAA-2021-001400 - Release_H13255_SHPO_Feature_Report_RedactedS.pdf",
         "development_and_tools/bagfiles/H13255_MB_50cm_LWD_3of6.bag",
         "H13255 - Straits of Mackinac"),
    ]
    
    # Known wrecks for reference
    known_wrecks = {
        "Elva": (45.849306, -84.613028),
        "Elva Barge": (45.849194, -84.612333),
        "Cedarville": (45.8175, -84.6058),
        "Nordmeer": (45.8181, -84.6047),
    }
    
    all_results = []
    
    for pdf_path, bag_path, survey_name in pdf_bag_pairs:
        print(f"\n{'='*80}")
        print(f"Survey: {survey_name}")
        print(f"PDF: {pdf_path}")
        print(f"BAG: {bag_path}")
        print("=" * 80)
        
        # Extract coordinates from PDF
        coords = extract_pdf_coordinates(pdf_path)
        print(f"\nExtracted {len(coords)} unique coordinates from PDF")
        
        # Open BAG file
        try:
            with rasterio.open(bag_path) as src:
                crs = src.crs
                pixel_size_ft = abs(src.transform[0]) * M_TO_FT
                transformer = Transformer.from_crs("EPSG:4326", crs, always_xy=True)
                
                print(f"BAG pixel size: {pixel_size_ft:.2f} ft")
                print(f"\nAnalyzing {len(coords)} locations...")
                print()
                
                # Categorize results
                wreck_sigs = []
                scrubbed = []
                high_texture = []
                normal = []
                no_data = []
                
                for coord in coords:
                    result = analyze_bag_location(src, coord['lat'], coord['lon'], transformer)
                    
                    # Check if near known wreck
                    nearest_wreck = None
                    min_dist_nm = float('inf')
                    for name, (wlat, wlon) in known_wrecks.items():
                        dist_nm = ((coord['lat']-wlat)**2 + (coord['lon']-wlon)**2)**0.5 * 60
                        if dist_nm < min_dist_nm:
                            min_dist_nm = dist_nm
                            nearest_wreck = name
                    
                    if result:
                        result['lat'] = coord['lat']
                        result['lon'] = coord['lon']
                        result['page'] = coord['page']
                        result['nearest_wreck'] = nearest_wreck if min_dist_nm < 0.5 else None
                        result['dist_nm'] = round(min_dist_nm, 2)
                        
                        if result['signature'] == "WRECK_SIGNATURE":
                            wreck_sigs.append(result)
                        elif result['signature'] == "SCRUBBED":
                            scrubbed.append(result)
                        elif result['signature'] == "HIGH_TEXTURE":
                            high_texture.append(result)
                        else:
                            normal.append(result)
                    else:
                        no_data.append(coord)
                
                # Print results by category
                print("=" * 80)
                print("WRECK SIGNATURES DETECTED (%d locations)" % len(wreck_sigs))
                print("=" * 80)
                for r in wreck_sigs:
                    wreck_note = f" ** {r['nearest_wreck']}" if r['nearest_wreck'] else ""
                    print(f"  {r['lat']:.5f}, {r['lon']:.5f} (pg {r['page']}){wreck_note}")
                    print(f"    Depth: {r['depth_ft']} ft | Std: {r['std_ft']} ft | GradMax: {r['grad_max_ft']} ft")
                    print(f"    Extent: {r['extent_ft'][0]:.0f} x {r['extent_ft'][1]:.0f} ft | Aspect: {r['aspect_ratio']}")
                
                print("\n" + "=" * 80)
                print("SCRUBBED AREAS DETECTED (%d locations)" % len(scrubbed))
                print("=" * 80)
                for r in scrubbed:
                    wreck_note = f" ** HIDDEN {r['nearest_wreck']}?" if r['nearest_wreck'] else ""
                    print(f"  {r['lat']:.5f}, {r['lon']:.5f} (pg {r['page']}){wreck_note}")
                    print(f"    Depth: {r['depth_ft']} ft | Std: {r['std_ft']} ft (very low!)")
                
                print("\n" + "=" * 80)
                print("HIGH TEXTURE AREAS (%d locations)" % len(high_texture))
                print("=" * 80)
                for r in high_texture[:10]:  # Show first 10
                    wreck_note = f" ** {r['nearest_wreck']}" if r['nearest_wreck'] else ""
                    print(f"  {r['lat']:.5f}, {r['lon']:.5f} | Depth: {r['depth_ft']} ft | Grad: {r['grad_mean_ft']}{wreck_note}")
                if len(high_texture) > 10:
                    print(f"  ... and {len(high_texture) - 10} more")
                
                print("\n" + "=" * 80)
                print("SUMMARY")
                print("=" * 80)
                print(f"  Total PDF coordinates analyzed: {len(coords)}")
                print(f"  WRECK SIGNATURES: {len(wreck_sigs)}")
                print(f"  SCRUBBED (hidden wrecks?): {len(scrubbed)}")
                print(f"  High texture: {len(high_texture)}")
                print(f"  Normal terrain: {len(normal)}")
                print(f"  No BAG data: {len(no_data)}")
                
                # Store results
                all_results.append({
                    'survey': survey_name,
                    'wreck_signatures': wreck_sigs,
                    'scrubbed': scrubbed,
                    'high_texture': high_texture,
                    'normal': normal
                })
                
        except Exception as e:
            print(f"Error processing BAG: {e}")
    
    return all_results


if __name__ == "__main__":
    results = main()
