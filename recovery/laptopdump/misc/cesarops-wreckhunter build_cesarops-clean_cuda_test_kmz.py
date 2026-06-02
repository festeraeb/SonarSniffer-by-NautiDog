#!/usr/bin/env python3
"""
CUDA Test Suite with KMZ Export
Tests Quadro M2200 CUDA cores and exports results to Google Earth KMZ
Logs results to LAKE_MICHIGAN_CENSUS_2026.db
"""

import numpy as np
from pathlib import Path
import json
from datetime import datetime
import simplekml

# Import database connector
from database_connector import log_scan_run_to_census, get_census_db

def check_cuda():
    """Check CUDA availability and GPU info"""
    try:
        import cupy as cp
        import cupy.cuda.runtime as runtime
        
        device_id = 0
        props = runtime.getDeviceProperties(device_id)
        gpu_name = props['name'].decode('utf-8')
        compute_cap = f"{props['major']}.{props['minor']}"
        total_memory = props['totalGlobalMem'] // (1024**2)  # MB
        
        return {
            'available': True,
            'gpu_name': gpu_name,
            'compute_capability': compute_cap,
            'total_memory_mb': total_memory,
            'multiprocessor_count': props['multiProcessorCount'],
            'clock_rate_mhz': props['clockRate'] // 1000
        }
    except Exception as e:
        return {
            'available': False,
            'error': str(e)
        }

def run_cuda_benchmark():
    """Run CUDA benchmark tests on M2200"""
    import cupy as cp
    import time
    
    results = {}
    
    # Test 1: Matrix multiplication
    print("  [1/4] Matrix multiplication (1024x1024)...")
    size = 1024
    a = cp.random.rand(size, size, dtype=cp.float32)
    b = cp.random.rand(size, size, dtype=cp.float32)
    
    start = time.time()
    c = cp.dot(a, b)
    c = c.get()  # Sync
    elapsed = time.time() - start
    results['matmul_1024_ms'] = round(elapsed * 1000, 2)
    
    # Test 2: Z-score computation
    print("  [2/4] Z-score computation (2048x2048)...")
    size = 2048
    data = cp.random.rand(size, size, dtype=cp.float32) * 100 + 50
    
    start = time.time()
    mean_val = cp.mean(data)
    std_val = cp.std(data)
    zscore = (data - mean_val) / std_val
    anomalies = cp.abs(zscore) > 2.5
    count = cp.sum(anomalies)
    elapsed = time.time() - start
    results['zscore_2048_ms'] = round(elapsed * 1000, 2)
    results['anomaly_count'] = int(count)
    
    # Test 3: Memory bandwidth
    print("  [3/4] Memory bandwidth test...")
    size_mb = 256
    data = cp.zeros(size_mb * 1024 * 1024 // 4, dtype=cp.float32)  # 256 MB

    start = time.time()
    for _ in range(10):
        data = data + 1
    _ = cp.sum(data)
    elapsed = time.time() - start
    if elapsed > 0:
        bandwidth = (size_mb * 2 * 10) / elapsed  # Read + Write * 10 iterations
        results['memory_bandwidth_mbps'] = round(bandwidth, 2)
    else:
        results['memory_bandwidth_mbps'] = 0
    
    # Test 4: Convolution-like operation (using scipy on GPU arrays)
    print("  [4/4] Filtering operation (512x512)...")
    size = 512
    img = cp.random.rand(size, size, dtype=cp.float32)
    
    start = time.time()
    for _ in range(10):
        # Gaussian blur using element-wise operations
        blurred = (cp.roll(img, 1, axis=0) + cp.roll(img, -1, axis=0) + 
                   cp.roll(img, 1, axis=1) + cp.roll(img, -1, axis=1) + 
                   img * 4) / 8
    _ = cp.sum(blurred)  # Sync
    elapsed = time.time() - start
    results['filter_10iter_ms'] = round(elapsed * 1000, 2)
    
    return results

def create_test_kmz(output_path: Path, gpu_info: dict, benchmark_results: dict, anomaly_coords: list = None):
    """Create KMZ file with test results"""
    kml = simplekml.Kml()
    
    # Add metadata
    kml.document.name = f"CUDA Test Results - {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}"
    
    # GPU Info folder
    gpu_folder = kml.newfolder(name="GPU Information")
    
    # Add GPU info as placemark description
    gpu_placemark = gpu_folder.newpoint(name=gpu_info.get('gpu_name', 'Unknown GPU'))
    gpu_placemark.description = f"""
    <h2>CUDA Test Results</h2>
    <table>
        <tr><td><b>GPU Name:</b></td><td>{gpu_info.get('gpu_name', 'N/A')}</td></tr>
        <tr><td><b>Compute Capability:</b></td><td>{gpu_info.get('compute_capability', 'N/A')}</td></tr>
        <tr><td><b>Total Memory:</b></td><td>{gpu_info.get('total_memory_mb', 'N/A')} MB</td></tr>
        <tr><td><b>Multiprocessors:</b></td><td>{gpu_info.get('multiprocessor_count', 'N/A')}</td></tr>
        <tr><td><b>Clock Rate:</b></td><td>{gpu_info.get('clock_rate_mhz', 'N/A')} MHz</td></tr>
    </table>
    
    <h3>Benchmark Results</h3>
    <table>
        <tr><td><b>Matrix Mul (1024x1024):</b></td><td>{benchmark_results.get('matmul_1024_ms', 'N/A')} ms</td></tr>
        <tr><td><b>Z-Score (2048x2048):</b></td><td>{benchmark_results.get('zscore_2048_ms', 'N/A')} ms</td></tr>
        <tr><td><b>Memory Bandwidth:</b></td><td>{benchmark_results.get('memory_bandwidth_mbps', 'N/A')} MB/s</td></tr>
        <tr><td><b>Filter (10 iter):</b></td><td>{benchmark_results.get('filter_10iter_ms', 'N/A')} ms</td></tr>
    </table>
    """
    # Set to Lake Michigan center for visualization
    gpu_placemark.coords = [(-86.0, 43.0)]
    gpu_placemark.style.iconstyle.icon.href = 'http://maps.google.com/mapfiles/kml/paddle/red-circle.png'
    
    # Anomalies folder (if any)
    if anomaly_coords:
        anomaly_folder = kml.newfolder(name="Detected Anomalies")
        
        for i, coord in enumerate(anomaly_coords[:50], 1):  # Limit to 50
            placemark = anomaly_folder.newpoint(name=f"Anomaly {i}")
            placemark.coords = [(coord['lon'], coord['lat'])]
            placemark.description = f"""
                <h3>Anomaly #{i}</h3>
                <p><b>Z-Score:</b> {coord['zscore']:.2f}</p>
                <p><b>Location:</b> Row {coord['row']}, Col {coord['col']}</p>
            """
            placemark.style.iconstyle.icon.href = 'http://maps.google.com/mapfiles/kml/paddle/blu-circle.png'
            placemark.style.iconstyle.scale = 0.8
    
    # Summary folder
    summary_folder = kml.newfolder(name="Test Summary")
    summary_placemark = summary_folder.newpoint(name="Test Summary")
    summary_placemark.coords = [(-86.0, 43.0)]
    summary_placemark.description = f"""
    <h2>CUDA Test Summary</h2>
    <p><b>Test Date:</b> {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}</p>
    <p><b>GPU:</b> {gpu_info.get('gpu_name', 'N/A')}</p>
    <p><b>Status:</b> {'✓ PASSED' if gpu_info.get('available') else '✗ FAILED'}</p>
    <p><b>Files Processed:</b> {benchmark_results.get('files_processed', 0)}</p>
    <p><b>Total Anomalies:</b> {benchmark_results.get('total_anomalies', 0)}</p>
    """
    summary_placemark.style.iconstyle.icon.href = 'http://maps.google.com/mapfiles/kml/paddle/grn-circle.png'
    
    # Save KMZ
    output_path.parent.mkdir(parents=True, exist_ok=True)
    kml.savekmz(str(output_path))
    print(f"  ✓ KMZ saved: {output_path}")

def main():
    print("=" * 80)
    print("CUDA TEST SUITE WITH KMZ EXPORT")
    print("=" * 80)
    print()
    
    # Step 1: Check CUDA
    print("[1/4] Checking CUDA availability...")
    gpu_info = check_cuda()
    
    if gpu_info['available']:
        print(f"  ✓ GPU: {gpu_info['gpu_name']}")
        print(f"  ✓ Compute Capability: {gpu_info['compute_capability']}")
        print(f"  ✓ Memory: {gpu_info['total_memory_mb']} MB")
        print(f"  ✓ CUDA Cores (SMs): {gpu_info['multiprocessor_count']}")
    else:
        print(f"  ✗ CUDA not available: {gpu_info['error']}")
        print("  Make sure CuPy is installed: pip install cupy-cuda11x")
        return
    
    print()
    
    # Step 2: Run benchmarks
    print("[2/4] Running CUDA benchmarks...")
    try:
        benchmark_results = run_cuda_benchmark()
        print(f"  ✓ Matrix multiplication: {benchmark_results['matmul_1024_ms']} ms")
        print(f"  ✓ Z-score computation: {benchmark_results['zscore_2048_ms']} ms")
        print(f"  ✓ Memory bandwidth: {benchmark_results['memory_bandwidth_mbps']} MB/s")
        print(f"  ✓ Filter operation: {benchmark_results['filter_10iter_ms']} ms")
    except Exception as e:
        print(f"  ✗ Benchmark failed: {e}")
        benchmark_results = {'error': str(e)}
    
    print()
    
    # Step 3: Process sample data (if available)
    print("[3/4] Processing sample data...")
    anomaly_coords = []
    total_anomalies = 0
    files_processed = 0
    
    # Try to find real TIFF files
    search_paths = [
        Path(r"C:\Users\thomf\programming\Bagrecovery\outputs\rossa_forensic_cache"),
        Path(r"C:\Users\thomf\programming\Bagrecovery\sentinel_hunt\cache"),
        Path(r"C:\Users\thomf\programming\cesarops-wreckhunter build\outputs"),
    ]
    
    tiffs = []
    for search_path in search_paths:
        if search_path.exists():
            tiffs.extend(search_path.rglob("*B01.tif"))
            tiffs.extend(search_path.rglob("*B02.tif"))
            tiffs.extend(search_path.rglob("*B11.tif"))
            tiffs.extend(search_path.rglob("*B12.tif"))
    
    tiffs = sorted(set(tiffs))
    
    if tiffs:
        print(f"  Found {len(tiffs)} TIFF files to process")
        
        import cupy as cp
        from PIL import Image
        
        for i, tiff in enumerate(tiffs[:30], 1):  # Process up to 30 files
            print(f"  [{i}/{min(len(tiffs), 30)}] {tiff.name}...")
            try:
                img = Image.open(tiff)
                data = np.array(img, dtype=np.float32)
                
                # Upload to GPU
                data_gpu = cp.asarray(data)
                mean_val = cp.mean(data_gpu)
                std_val = cp.std(data_gpu)
                zscore = (data_gpu - mean_val) / std_val
                anomalies = cp.abs(zscore) > 2.5
                
                file_anomalies = int(cp.sum(anomalies))
                total_anomalies += file_anomalies
                files_processed += 1
                print(f"      -> {file_anomalies} anomalies")
                
                # Get coordinates for top anomalies
                if file_anomalies > 0:
                    anomaly_indices = cp.where(anomalies)
                    rows = cp.asnumpy(anomaly_indices[0])[:5]  # Top 5 per file
                    cols = cp.asnumpy(anomaly_indices[1])[:5]
                    zscores = cp.asnumpy(zscore[anomalies])[:5]
                    
                    # Mock coordinates (Lake Michigan center)
                    base_lat, base_lon = 43.0, -86.0
                    for r, c, z in zip(rows, cols, zscores):
                        anomaly_coords.append({
                            'row': int(r),
                            'col': int(c),
                            'zscore': float(z),
                            'lat': base_lat + (r - data.shape[0]//2) * 0.001,
                            'lon': base_lon + (c - data.shape[1]//2) * 0.001,
                            'source': tiff.name
                        })
            except Exception as e:
                print(f"      ERROR: {e}")
        
        print(f"  ✓ Processed {files_processed} files, {total_anomalies} total anomalies")
        benchmark_results['files_processed'] = files_processed
        benchmark_results['total_anomalies'] = total_anomalies
    else:
        print("  ⚠ No TIFF files found, skipping data processing")
    
    print()
    
    # Step 4: Export to KMZ
    print("[4/4] Exporting results to KMZ...")
    output_dir = Path("outputs") / "cuda_tests"
    timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
    kmz_path = output_dir / f"cuda_test_results_{timestamp}.kmz"
    
    create_test_kmz(kmz_path, gpu_info, benchmark_results, anomaly_coords)

    # Log to database
    print("\n[DB] Logging results to census database...")
    try:
        log_scan_run_to_census(
            run_name=f"cuda_test_{datetime.now().strftime('%Y%m%d_%H%M%S')}",
            tile_count=files_processed,
            detection_count=total_anomalies,
            notes=f"Quadro M2200 - {benchmark_results.get('matmul_1024_ms', 0)}ms matmul"
        )
        print("  ✓ Logged to database")
    except Exception as e:
        print(f"  ✗ Database logging failed: {e}")

    print()
    print("=" * 80)
    print("TEST COMPLETE")
    print("=" * 80)
    print(f"  KMZ: {kmz_path.absolute()}")
    print()
    print("Open in Google Earth to view results")
    print("=" * 80)

if __name__ == "__main__":
    main()
