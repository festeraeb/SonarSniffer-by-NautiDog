#!/bin/bash
# XENON STARTUP SCRIPT
# Run this on Xenon (10.0.0.55) via SSH

echo "========================================"
echo "XENON - CESAROPS PROCESSING"
echo "========================================"
echo ""

# Check CUDA
echo "[1/5] Checking CUDA..."
python3 -c "import cupy; print('CuPy:', cupy.__version__); print('CUDA Devices:', cupy.cuda.runtime.getDeviceCount())"
echo ""

# Check database
echo "[2/5] Checking database..."
if [ -f "wreckhunter2000/LAKE_MICHIGAN_CENSUS_2026.db" ]; then
    echo "  Database found"
else
    echo "  Database NOT FOUND"
fi
echo ""

# Check for GeoTIFFs
echo "[3/5] Counting GeoTIFFs..."
TILE_COUNT=$(find wreckhunter2000/data/cache -name "*.tif" -size +100k | wc -l)
echo "  Found: $TILE_COUNT tiles"
echo ""

# Process tiles
echo "[4/5] Processing tiles..."
cd scripts
python3 process_tiles.py xenon
echo ""

# Start TPU server (optional)
echo "[5/5] Starting TPU server..."
python3 tpu_server.py &
echo "  TPU server started on port 5001"
echo ""

echo "========================================"
echo "XENON READY"
echo "========================================"
