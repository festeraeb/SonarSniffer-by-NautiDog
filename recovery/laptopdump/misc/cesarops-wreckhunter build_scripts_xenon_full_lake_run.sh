#!/bin/bash
# XENON - FULL LAKE MICHIGAN RUN
# Run this on Xenon (10.0.0.55) via SSH

echo "========================================"
echo "XENON - FULL LAKE MICHIGAN PROCESSING"
echo "========================================"
echo ""

# Check CUDA
echo "[1/5] Checking CUDA..."
python3 -c "import cupy; print('CuPy:', cupy.__version__); print('CUDA Devices:', cupy.cuda.runtime.getDeviceCount())"
if [ $? -ne 0 ]; then
    echo "ERROR: CUDA not working!"
    exit 1
fi
echo ""

# Check database
echo "[2/5] Checking database..."
if [ -f "wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db" ]; then
    echo "  Database: OK"
else
    echo "  Database: NOT FOUND"
fi
echo ""

# Count tiles
echo "[3/5] Counting GeoTIFFs..."
TILE_COUNT=$(find wreckhunter2000/data/cache -name "*.tif" -size +100k | wc -l)
echo "  Found: $TILE_COUNT tiles"
echo ""

# Run full processing
echo "[4/5] Running full lake processing..."
cd scripts
python3 full_lake_michigan_run.py
if [ $? -ne 0 ]; then
    echo "ERROR: Processing failed!"
    exit 1
fi
echo ""

# Generate oil KMZ
echo "[5/5] Generating oil spill KMZ..."
python3 extract_oil_spills_kmz.py
echo ""

echo "========================================"
echo "XENON PROCESSING COMPLETE"
echo "========================================"
echo ""
echo "Results saved to: outputs/full_lake_run/"
echo ""
