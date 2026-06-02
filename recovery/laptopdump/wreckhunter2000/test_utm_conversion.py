"""
Test UTM to lat/lon conversion to verify coordinates are correct.

This tests the full pipeline:
1. Rasterio transform (affine) - pixel to UTM
2. Pyproj transform - UTM to WGS84 lat/lon
"""

import rasterio
from pyproj import Transformer
from pathlib import Path

# Test with a known BAG file
bag_files = [
    r"bag_processor\restored_H13255_MB_1m_LWD_5of6_aligned.bag",
    r"bag_processor\restored_H13255_MB_4m_LWD_6of6_corrected.bag",
    r"bag_processor\F00542_VB_2m_LWD_1of2.bag",  # Cedarville area
]

for bag_path in bag_files:
    if not Path(bag_path).exists():
        print(f"File not found: {bag_path}")
        continue
    
    print(f"\n{'='*70}")
    print(f"Testing: {Path(bag_path).name}")
    print(f"{'='*70}")
    
    with rasterio.open(bag_path) as src:
        transform = src.transform
        crs = src.crs
        
        print(f"CRS: {crs}")
        print(f"Transform (affine):")
        print(f"  a={transform.a:.6f}, b={transform.b:.6f}, c={transform.c:.6f}")
        print(f"  d={transform.d:.6f}, e={transform.e:.6f}, f={transform.f:.6f}")
        print()
        
        # Test center of image
        rows, cols = src.shape
        center_row, center_col = rows // 2, cols // 2
        
        # Method 1: Using rasterio's transform directly
        utm_x = transform.c + center_col * transform.a + center_row * transform.b
        utm_y = transform.f + center_col * transform.d + center_row * transform.e
        
        print(f"Image center: row={center_row}, col={center_col}")
        print(f"  UTM (from affine):  E={utm_x:.3f}, N={utm_y:.3f}")
        
        # Method 2: Using rasterio's xy() helper
        rasterio_x, rasterio_y = src.xy(center_row, center_col, offset='center')
        print(f"  UTM (rasterio.xy):  E={rasterio_x:.3f}, N={rasterio_y:.3f}")
        
        # Convert to lat/lon
        transformer = Transformer.from_crs(crs, "EPSG:4326", always_xy=True)
        lon, lat = transformer.transform(utm_x, utm_y)
        print(f"  WGS84 lat/lon:      {lat:.6f}, {lon:.6f}")
        
        # Test a known target location (Cedarville wreck)
        print()
        print("Testing known target (Cedarville wreck approx):")
        cedarville_lat, cedarville_lon = 45.78725, -84.6708
        print(f"  Expected WGS84: {cedarville_lat:.6f}, {cedarville_lon:.6f}")
        
        # Check if this point is in our file bounds
        if src.bounds.left <= utm_x <= src.bounds.right and src.bounds.bottom <= utm_y <= src.bounds.top:
            print(f"  ✓ Center point is within file bounds")
        else:
            print(f"  ✗ Center point is OUTSIDE file bounds")
            print(f"    Bounds: E[{src.bounds.left:.3f}, {src.bounds.right:.3f}], "
                  f"N[{src.bounds.bottom:.3f}, {src.bounds.top:.3f}]")
        
        # Reverse test: convert known lat/lon to UTM and check pixel
        print()
        print("Reverse test - known lat/lon to pixel:")
        reverse_transformer = Transformer.from_crs("EPSG:4326", crs, always_xy=True)
        target_easting, target_northing = reverse_transformer.transform(cedarville_lon, cedarville_lat)
        print(f"  UTM for Cedarville: E={target_easting:.3f}, N={target_northing:.3f}")
        
        # Check if in bounds
        if src.bounds.left <= target_easting <= src.bounds.right and src.bounds.bottom <= target_northing <= src.bounds.top:
            row, col = src.index(target_easting, target_northing)
            print(f"  ✓ In bounds! Pixel: row={row}, col={col}")
            
            # Verify round-trip
            round_trip_x, round_trip_y = src.xy(row, col, offset='center')
            round_trip_lon, round_trip_lat = transformer.transform(round_trip_x, round_trip_y)
            print(f"  Round-trip: {round_trip_lat:.6f}, {round_trip_lon:.6f}")
            print(f"  Error: dLat={abs(round_trip_lat - cedarville_lat)*1e6:.1f}µ°, "
                  f"dLon={abs(round_trip_lon - cedarville_lon)*1e6:.1f}µ°")
        else:
            print(f"  ✗ Target is OUTSIDE this file's bounds")
