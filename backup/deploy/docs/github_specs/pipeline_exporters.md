# Exporters & Pipeline

SonarSniffer includes a flexible pipeline to export sonar data into multiple formats.

Supported exporters (initial):
- waterfall PNG (`waterfall`)
- MP4 video (`mp4`)
- KML ground overlay referencing waterfall (`kml`)
- GeoTIFF (if `rasterio` available) or PNG+TFW fallback (`geotiff`)
- Tile pyramid (`tiles`) — simple super-overlay style tiles (z/x/y.png)
- MBTiles (`mbtiles`) — packaged tiles in MBTiles SQLite format
- KMZ super-overlay (`kmz`) — KML/KMZ packaged overlays referencing tiles for fast loading
- Mosaic builder (`mosaic`) — simple average overlay of available PNGs

Notes on performance and memory usage:
- Tile generation is streamed and writes tiles to disk per tile; the MBTiles writer streams tile blobs into SQLite to avoid holding many images in memory.
- The KMZ generator references tile images and writes a compact `doc.kml` that GroundOverlays clients can load incrementally for fast startup.
- For production, consider using GDAL's tile and MBTiles utilities (e.g., `gdal2tiles.py`) for more optimized and canonical MBTiles output.

Usage (CLI):

    python -m src.sonarsniffer.cli export-full <file> --output=outputs --format=waterfall,kml,geotiff,tiles

Notes:
- Exporters are designed to be defensive: missing optional deps (e.g., `rasterio`) fall back to safe, non-failing behavior.
- The pipeline uses `Scan` canonical objects and adapter registration — adding new parsers is straightforward: implement an adapter that converts parser records to `Scan` and register it.
