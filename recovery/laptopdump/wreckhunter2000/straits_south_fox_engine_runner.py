"""
straits_south_fox_engine_runner.py

Preprocesses every downloaded HDF5 file from the Straits/South Fox pull
and passes each one through the Rust GPU engine (cesarops-gpu.exe).

Pipeline per file:
  1. Open HDF5 (VIIRS LST or DNB)
  2. Extract science dataset, apply scale factor
  3. Read real tile bounds from StructMetadata (sinusoidal projection)
  4. Clip to Straits/South Fox bbox
  5. Write float32 GeoTIFF the Rust engine can read
  6. Call cesarops-gpu.exe <tiff> --threshold <t>
  7. Parse stdout for anomaly hits
  8. Accumulate all hits into master JSON + KML

Confirmed HDF5 paths (from file inspection):
  VNP21A1D: HDFEOS/GRIDS/VIIRS_Grid_Daily_1km_LST21/Data Fields/LST_1KM
  VNP46A1:  HDFEOS/GRIDS/VIIRS_Grid_DNB_2d/Data Fields/BrightnessTemperature_M15
"""

import subprocess
import json
import re
import math
from pathlib import Path
from datetime import datetime, date, timedelta

try:
    import h5py
    HAS_H5PY = True
except ImportError:
    HAS_H5PY = False
    print('[!] h5py not installed: pip install h5py')

try:
    import numpy as np
    HAS_NUMPY = True
except ImportError:
    HAS_NUMPY = False
    print('[!] numpy not installed: pip install numpy')

try:
    import rasterio
    from rasterio.transform import from_bounds
    HAS_RASTERIO = True
except ImportError:
    HAS_RASTERIO = False
    print('[!] rasterio not installed: pip install rasterio')

# ── Paths ─────────────────────────────────────────────────────────────────────
REPO       = Path(__file__).resolve().parent
INPUT_ROOT = REPO / 'outputs' / 'straits_south_fox_historical'
OUTPUT_DIR = INPUT_ROOT / 'engine_results'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

GPU_EXE = REPO.parent / 'target' / 'release' / 'cesarops-gpu.exe'
if not GPU_EXE.exists():
    GPU_EXE = REPO / 'target' / 'release' / 'cesarops-gpu.exe'
if not GPU_EXE.exists():
    # Try parent's parent (for wreckhunter2000 subfolder)
    GPU_EXE = REPO.parent.parent / 'target' / 'release' / 'cesarops-gpu.exe'

# ── Bbox ──────────────────────────────────────────────────────────────────────
BBOX = {
    'lon_min': -86.10, 'lat_min': 45.35,
    'lon_max': -85.35, 'lat_max': 46.00,
}

# ── Sensor configs ─────────────────────────────────────────────────────────────
SENSOR_CONFIGS = {
    'VNP21A1D': {
        'label'     : 'VIIRS LST 1km',
        'h5_paths'  : [
            'HDFEOS/GRIDS/VIIRS_Grid_Daily_1km_LST21/Data Fields/LST_1KM',
            'HDFEOS/GRIDS/VIIRS_Grid_Daily_1km_LST/Data Fields/LST_1KM',
        ],
        'scale'     : 0.02,    # DN * 0.02 = Kelvin
        'fill_value': 0,
        'valid_min' : 7500,    # 150K minimum
        'valid_max' : 65535,
        'threshold' : 1.0,
        'mode'      : 'cold_sink',
        'note'      : 'Cold-sink: steel wreck holds 4C while lake surface warms',
    },
    'VNP46A1': {
        'label'     : 'VIIRS DNB M15 Thermal',
        'h5_paths'  : [
            'HDFEOS/GRIDS/VIIRS_Grid_DNB_2d/Data Fields/BrightnessTemperature_M15',
            'HDFEOS/GRIDS/VIIRS_Grid_DNB_2d/Data Fields/DNB_At_Sensor_Radiance',
        ],
        'scale'     : 0.1,     # BT_M15: DN * 0.1 = Kelvin
        'fill_value': 65535,
        'valid_min' : 0,
        'valid_max' : 65534,
        'threshold' : 1.0,
        'mode'      : 'thermal_contrast',
        'note'      : 'M15 11um thermal: nighttime cold-sink from steel wrecks',
    },
}

# ── Helpers ────────────────────────────────────────────────────────────────────

def parse_tile_hv(filename):
    """Extract h and v tile indices from VIIRS filename."""
    m = re.search(r'\.h(\d{2})v(\d{2})\.', filename)
    if m:
        return int(m.group(1)), int(m.group(2))
    return None, None


def parse_doy_year(filename):
    """Extract year and day-of-year from VIIRS filename like VNP21A1D.A2012092."""
    m = re.search(r'\.A(\d{4})(\d{3})\.', filename)
    if m:
        return int(m.group(1)), int(m.group(2))
    return None, None


def doy_to_date(year, doy):
    """Convert year + day-of-year to ISO date string."""
    try:
        return (date(year, 1, 1) + timedelta(days=doy - 1)).isoformat()
    except Exception:
        return f'{year}-DOY{doy:03d}'


def get_tile_bounds(h5_file):
    """
    Read actual tile geographic bounds from VIIRS HDF5 StructMetadata.
    Handles two projection types:
      HE5_GCTP_SNSOID (sinusoidal) - VNP21A1D LST: coords in meters
      HE5_GCTP_GEO    (geographic)  - VNP46A1  DNB: coords in degrees * 1e6
    Returns dict with lon_min, lon_max, lat_min, lat_max or None.
    """
    R = 6371007.181
    try:
        raw = h5_file['HDFEOS INFORMATION/StructMetadata.0'][()]
        s = raw.tobytes().decode('utf-8', 'replace') if hasattr(raw, 'tobytes') else str(raw)

        ul = re.search(r'UpperLeftPointMtrs=\(([^,]+),([^)]+)\)', s)
        lr = re.search(r'LowerRightMtrs=\(([^,]+),([^)]+)\)', s)
        if not ul or not lr:
            return None

        ul_x, ul_y = float(ul.group(1)), float(ul.group(2))
        lr_x, lr_y = float(lr.group(1)), float(lr.group(2))

        # Detect projection
        is_geo = 'HE5_GCTP_GEO' in s

        if is_geo:
            # Geographic projection: values are degrees * 1e6
            ul_lon = ul_x / 1e6
            ul_lat = ul_y / 1e6
            lr_lon = lr_x / 1e6
            lr_lat = lr_y / 1e6
        else:
            # Sinusoidal projection: values are meters
            def sinu_to_ll(x, y):
                lat = math.degrees(y / R)
                lon = math.degrees(x / (R * math.cos(math.radians(lat))))
                return lat, lon
            ul_lat, ul_lon = sinu_to_ll(ul_x, ul_y)
            lr_lat, lr_lon = sinu_to_ll(lr_x, lr_y)

        return {
            'lon_min': min(ul_lon, lr_lon),
            'lon_max': max(ul_lon, lr_lon),
            'lat_min': min(ul_lat, lr_lat),
            'lat_max': max(ul_lat, lr_lat),
        }
    except Exception:
        return None

# ── HDF5 Loader ────────────────────────────────────────────────────────────────

def load_viirs_h5(h5_path, sensor_key):
    """
    Load VIIRS HDF5, extract science dataset, apply scale.
    Returns tile_info dict or None on failure.
    """
    if not HAS_H5PY or not HAS_NUMPY:
        return None

    cfg   = SENSOR_CONFIGS[sensor_key]
    fname = h5_path.name
    h_tile, v_tile = parse_tile_hv(fname)
    year, doy      = parse_doy_year(fname)

    try:
        with h5py.File(h5_path, 'r') as f:
            # Find dataset
            ds = None
            for path in cfg['h5_paths']:
                try:
                    ds = f[path]
                    break
                except KeyError:
                    continue

            if ds is None:
                # Walk tree as fallback
                target_name = cfg['h5_paths'][0].split('/')[-1]
                found_path  = None
                def _visit(name, obj):
                    nonlocal found_path
                    if found_path:
                        return
                    if isinstance(obj, h5py.Dataset) and target_name in name:
                        found_path = name
                f.visititems(_visit)
                if found_path:
                    ds = f[found_path]
                else:
                    print(f'    [!] Dataset not found in {fname}')
                    return None

            raw = ds[()].astype(np.float32)

            # Mask fill and out-of-range
            raw[raw == cfg['fill_value']] = np.nan
            raw[(raw < cfg['valid_min']) | (raw > cfg['valid_max'])] = np.nan
            data = raw * cfg['scale']

            # Get real tile bounds from file
            bounds = get_tile_bounds(f)

    except Exception as ex:
        print(f'    [!] HDF5 error {fname}: {ex}')
        return None

    nrows, ncols = data.shape

    if bounds:
        tlon_min = bounds['lon_min']
        tlon_max = bounds['lon_max']
        tlat_min = bounds['lat_min']
        tlat_max = bounds['lat_max']
    elif h_tile is not None:
        # Rough fallback (not accurate for sinusoidal)
        tlon_min = h_tile * 10.0 - 180.0
        tlon_max = tlon_min + 10.0
        tlat_max = 90.0 - v_tile * 10.0
        tlat_min = tlat_max - 10.0
    else:
        tlon_min, tlon_max = -180.0, 180.0
        tlat_min, tlat_max = -90.0, 90.0

    return {
        'data'        : data,
        'nrows'       : nrows,
        'ncols'       : ncols,
        'tile_lon_min': tlon_min,
        'tile_lon_max': tlon_max,
        'tile_lat_min': tlat_min,
        'tile_lat_max': tlat_max,
        'h_tile'      : h_tile,
        'v_tile'      : v_tile,
        'year'        : year,
        'doy'         : doy,
        'sensor_key'  : sensor_key,
    }

# ── Clip ───────────────────────────────────────────────────────────────────────

def clip_to_bbox(tile_info):
    """Clip tile to BBOX. Returns updated tile_info or None if no overlap."""
    data  = tile_info['data']
    nrows = tile_info['nrows']
    ncols = tile_info['ncols']
    tlon_min = tile_info['tile_lon_min']
    tlon_max = tile_info['tile_lon_max']
    tlat_min = tile_info['tile_lat_min']
    tlat_max = tile_info['tile_lat_max']

    if (tlon_max < BBOX['lon_min'] or tlon_min > BBOX['lon_max'] or
            tlat_max < BBOX['lat_min'] or tlat_min > BBOX['lat_max']):
        return None

    px_lon = (tlon_max - tlon_min) / ncols
    px_lat = (tlat_max - tlat_min) / nrows

    col_start = max(0, int((BBOX['lon_min'] - tlon_min) / px_lon))
    col_end   = min(ncols, int((BBOX['lon_max'] - tlon_min) / px_lon) + 1)
    row_start = max(0, int((tlat_max - BBOX['lat_max']) / px_lat))
    row_end   = min(nrows, int((tlat_max - BBOX['lat_min']) / px_lat) + 1)

    if col_end <= col_start or row_end <= row_start:
        return None

    clipped = data[row_start:row_end, col_start:col_end]

    return {
        **tile_info,
        'data'        : clipped,
        'nrows'       : clipped.shape[0],
        'ncols'       : clipped.shape[1],
        'clip_lon_min': tlon_min + col_start * px_lon,
        'clip_lon_max': tlon_min + col_end   * px_lon,
        'clip_lat_max': tlat_max - row_start * px_lat,
        'clip_lat_min': tlat_max - row_end   * px_lat,
    }

# ── GeoTIFF Writer ─────────────────────────────────────────────────────────────

def write_geotiff(tile_info, out_path):
    """Write clipped float32 array as GeoTIFF for the Rust engine."""
    data    = tile_info['data'].astype(np.float32)
    h, w    = data.shape
    lon_min = tile_info.get('clip_lon_min', tile_info['tile_lon_min'])
    lon_max = tile_info.get('clip_lon_max', tile_info['tile_lon_max'])
    lat_min = tile_info.get('clip_lat_min', tile_info['tile_lat_min'])
    lat_max = tile_info.get('clip_lat_max', tile_info['tile_lat_max'])

    if HAS_RASTERIO:
        try:
            transform = from_bounds(lon_min, lat_min, lon_max, lat_max, w, h)
            with rasterio.open(out_path, 'w', driver='GTiff',
                               height=h, width=w, count=1, dtype='float32',
                               crs='EPSG:4326', transform=transform,
                               nodata=float('nan')) as dst:
                dst.write(data, 1)
                # Write sidecar geo metadata so downstream tools (or Rust) can
                # pick up the exact geotransform and CRS when processing chunks.
                try:
                    # Prefer GDAL-style geotransform if available
                    gt = None
                    if hasattr(transform, 'to_gdal') and callable(transform.to_gdal):
                        gt = transform.to_gdal()
                    else:
                        gt = list(transform)
                    crs_str = dst.crs.to_string() if dst.crs else 'EPSG:4326'
                    geo = {
                        'geotransform': gt,
                        'crs': crs_str,
                        'width': w,
                        'height': h,
                    }
                    with open(out_path.with_suffix('.geo.json'), 'w', encoding='utf-8') as gf:
                        json.dump(geo, gf, indent=2)
                except Exception:
                    pass
            return True
        except Exception as ex:
            print(f'    [!] GeoTIFF write failed: {ex}')
            return False
    else:
        # Fallback: numpy binary + sidecar geo JSON
        np.save(out_path.with_suffix('.npy'), data)
        geo = {k: v for k, v in tile_info.items() if k != 'data'}
        with open(out_path.with_suffix('.geo.json'), 'w') as f:
            json.dump(geo, f, indent=2)
        return True

# ── Rust Engine ────────────────────────────────────────────────────────────────

def run_rust_engine(tiff_path, threshold):
    """Call cesarops-gpu.exe, parse anomaly lines from stdout."""
    if not GPU_EXE.exists():
        return [{'error': f'GPU exe not found: {GPU_EXE}'}]
    try:
        result = subprocess.run(
            [str(GPU_EXE), str(tiff_path), '--threshold', str(threshold)],
            capture_output=True, text=True, timeout=180
        )
        if result.returncode != 0:
            return [{'error': result.stderr[:200]}]

        anomalies = []
        for line in result.stdout.split('\n'):
            m = re.search(r'Pixel \((\d+),\s*(\d+)\):\s*Z-Score\s*([\d.\-]+)', line)
            if m:
                anomalies.append({
                    'row'   : int(m.group(1)),
                    'col'   : int(m.group(2)),
                    'zscore': float(m.group(3)),
                })
        return anomalies
    except subprocess.TimeoutExpired:
        return [{'error': 'timeout'}]
    except Exception as ex:
        return [{'error': str(ex)[:120]}]


def pixel_to_latlon(row, col, tile_info):
    """Convert pixel row/col to lat/lon."""
    lon_min = tile_info.get('clip_lon_min', tile_info['tile_lon_min'])
    lon_max = tile_info.get('clip_lon_max', tile_info['tile_lon_max'])
    lat_min = tile_info.get('clip_lat_min', tile_info['tile_lat_min'])
    lat_max = tile_info.get('clip_lat_max', tile_info['tile_lat_max'])
    nrows   = tile_info['nrows']
    ncols   = tile_info['ncols']
    lon = lon_min + (col + 0.5) * (lon_max - lon_min) / ncols
    lat = lat_max - (row + 0.5) * (lat_max - lat_min) / nrows
    return round(lat, 6), round(lon, 6)


def latlon_to_utm(lat, lon):
    """
    Convert WGS84 lat/lon to the correct UTM zone.
    Auto-selects zone 16T or 17T based on longitude.
    Zone 16T: lon -90 to -84
    Zone 17T: lon -84 to -78
    Returns (easting, northing, zone_number).
    """
    zone = 16 if lon < -84.0 else 17
    epsg = 32600 + zone  # 32616 or 32617

    try:
        from pyproj import Transformer
        t = Transformer.from_crs('EPSG:4326', f'EPSG:{epsg}', always_xy=True)
        easting, northing = t.transform(lon, lat)
        return round(easting, 1), round(northing, 1), zone
    except ImportError:
        pass

    # Simplified UTM formula fallback
    a    = 6378137.0
    f    = 1 / 298.257223563
    b    = a * (1 - f)
    e2   = 1 - (b / a) ** 2
    e_p2 = e2 / (1 - e2)
    k0   = 0.9996
    lon0 = math.radians(-87.0 if zone == 16 else -81.0)  # central meridians
    lat_r = math.radians(lat)
    lon_r = math.radians(lon)
    N = a / math.sqrt(1 - e2 * math.sin(lat_r) ** 2)
    T = math.tan(lat_r) ** 2
    C = e_p2 * math.cos(lat_r) ** 2
    A = math.cos(lat_r) * (lon_r - lon0)
    M = a * ((1 - e2/4 - 3*e2**2/64) * lat_r
             - (3*e2/8 + 3*e2**2/32) * math.sin(2*lat_r)
             + (15*e2**2/256) * math.sin(4*lat_r))
    easting  = k0 * N * (A + (1-T+C)*A**3/6) + 500000.0
    northing = k0 * (M + N*math.tan(lat_r)*(A**2/2 + (5-T+9*C)*A**4/24))
    return round(easting, 1), round(northing, 1), zone


# Keep old name as alias for existing callers
def latlon_to_utm16t(lat, lon):
    e, n, _ = latlon_to_utm(lat, lon)
    return e, n

# ── KML ────────────────────────────────────────────────────────────────────────

def write_kml(detections, out_path):
    """Write clustered detections to KML with color-coded confidence pins and toggleable folders."""
    lines = [
        '<?xml version="1.0" encoding="UTF-8"?>',
        '<kml xmlns="http://www.opengis.net/kml/2.2">',
        '<Document>',
        '  <name>Straits-South Fox Clustered Detections</name>',
        '  <description>VIIRS thermal detections clustered by proximity (1km) and Z-score similarity</description>',
        '',
        '  <!-- Warm Signatures -->',
        '  <Style id="warm_low"><IconStyle><color>ff00ffff</color><scale>0.8</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/ylw-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="warm_med"><IconStyle><color>ff0099ff</color><scale>1.0</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/orange-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="warm_high"><IconStyle><color>ff0000ff</color><scale>1.2</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="warm_very"><IconStyle><color>ff000088</color><scale>1.4</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/red-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '',
        '  <!-- Cold Signatures -->',
        '  <Style id="cold_low"><IconStyle><color>ffff0000</color><scale>0.8</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/blu-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="cold_med"><IconStyle><color>ffaa0000</color><scale>1.0</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/blu-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="cold_high"><IconStyle><color>ff880088</color><scale>1.2</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/purple-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '  <Style id="cold_very"><IconStyle><color>ff660066</color><scale>1.4</scale>',
        '    <Icon><href>http://maps.google.com/mapfiles/kml/paddle/purple-circle.png</href></Icon>',
        '  </IconStyle></Style>',
        '',
    ]
    
    # Group by folder
    warm_dets = [d for d in detections if d.get('temperature_class') == 'WARM']
    cold_dets = [d for d in detections if d.get('temperature_class') == 'COLD']
    
    if warm_dets:
        lines.append('  <Folder>')
        lines.append('    <name>Warm Thermal Signatures</name>')
        lines.append(f'    <description>{len(warm_dets)} warm anomalies (positive Z-scores)</description>')
        for d in warm_dets:
            conf = d.get('confidence', 'LOW').lower().replace('_', '')
            style = f'warm_{conf}'
            z = d.get('avg_zscore', d.get('zscore', 0))
            n = d.get('observation_count', 1)
            dates = d.get('observation_dates', [d.get('date', '')])
            sensors = d.get('sensors', [d.get('sensor', '')])
            utm = d.get('utm', {})
            lines += [
                '    <Placemark>',
                f'      <name>[{d.get("confidence","LOW")}] {n}x obs Z={z:.2f}</name>',
                f'      <styleUrl>#{style}</styleUrl>',
                '      <description><![CDATA[',
                f'        <b>Confidence:</b> {d.get("confidence","LOW")} ({n} observations)<br/>',
                f'        <b>Avg Z-Score:</b> {z:.3f}<br/>',
                f'        <b>Z-Range:</b> {d.get("zscore_range",[])[0]:.2f} to {d.get("zscore_range",[])[1]:.2f}<br/>',
                f'        <b>Temperature:</b> WARM (positive Z)<br/>',
                f'        <b>Sensors:</b> {", ".join(sensors)}<br/>',
                f'        <b>Dates:</b> {", ".join(dates)}<br/>',
                f'        <b>WGS84:</b> {d["lat"]}N, {d["lon"]}W<br/>',
                f'        <b>UTM-{utm.get("zone",16)}T:</b> E {utm.get("easting","?")} N {utm.get("northing","?")}<br/>',
                f'        <b>Grid Ref:</b> {d.get("grid_ref","")}<br/>',
                '      ]]></description>',
                f'      <Point><coordinates>{d["lon"]},{d["lat"]},0</coordinates></Point>',
                '    </Placemark>',
            ]
        lines.append('  </Folder>')
    
    if cold_dets:
        lines.append('  <Folder>')
        lines.append('    <name>Cold Thermal Signatures</name>')
        lines.append(f'    <description>{len(cold_dets)} cold anomalies (negative Z-scores)</description>')
        for d in cold_dets:
            conf = d.get('confidence', 'LOW').lower().replace('_', '')
            style = f'cold_{conf}'
            z = d.get('avg_zscore', d.get('zscore', 0))
            n = d.get('observation_count', 1)
            dates = d.get('observation_dates', [d.get('date', '')])
            sensors = d.get('sensors', [d.get('sensor', '')])
            utm = d.get('utm', {})
            lines += [
                '    <Placemark>',
                f'      <name>[{d.get("confidence","LOW")}] {n}x obs Z={z:.2f}</name>',
                f'      <styleUrl>#{style}</styleUrl>',
                '      <description><![CDATA[',
                f'        <b>Confidence:</b> {d.get("confidence","LOW")} ({n} observations)<br/>',
                f'        <b>Avg Z-Score:</b> {z:.3f}<br/>',
                f'        <b>Z-Range:</b> {d.get("zscore_range",[])[0]:.2f} to {d.get("zscore_range",[])[1]:.2f}<br/>',
                f'        <b>Temperature:</b> COLD (negative Z)<br/>',
                f'        <b>Sensors:</b> {", ".join(sensors)}<br/>',
                f'        <b>Dates:</b> {", ".join(dates)}<br/>',
                f'        <b>WGS84:</b> {d["lat"]}N, {d["lon"]}W<br/>',
                f'        <b>UTM-{utm.get("zone",16)}T:</b> E {utm.get("easting","?")} N {utm.get("northing","?")}<br/>',
                f'        <b>Grid Ref:</b> {d.get("grid_ref","")}<br/>',
                '      ]]></description>',
                f'      <Point><coordinates>{d["lon"]},{d["lat"]},0</coordinates></Point>',
                '    </Placemark>',
            ]
        lines.append('  </Folder>')
    
    lines += ['</Document>', '</kml>']
    out_path.write_text('\n'.join(lines), encoding='utf-8')

# ── Per-file pipeline ──────────────────────────────────────────────────────────

def process_file(h5_path, sensor_key):
    """Full pipeline for one HDF5 file. Returns list of detection dicts."""
    cfg   = SENSOR_CONFIGS[sensor_key]
    fname = h5_path.name
    print(f'  [{sensor_key}] {fname}')

    if not HAS_H5PY or not HAS_NUMPY:
        print('    [!] Missing h5py or numpy')
        return []

    tile_info = load_viirs_h5(h5_path, sensor_key)
    if tile_info is None:
        return []

    print(f'    Tile bounds: lon {tile_info["tile_lon_min"]:.1f} to {tile_info["tile_lon_max"]:.1f}  '
          f'lat {tile_info["tile_lat_min"]:.1f} to {tile_info["tile_lat_max"]:.1f}')

    clipped = clip_to_bbox(tile_info)
    if clipped is None:
        print('    [!] No overlap with bbox')
        return []

    valid_px = int(np.sum(~np.isnan(clipped['data'])))
    print(f'    Clipped: {clipped["nrows"]}x{clipped["ncols"]} px  valid={valid_px}')

    if valid_px < 10:
        print('    [!] Too few valid pixels')
        return []

    tiff_path = OUTPUT_DIR / fname.replace('.h5', '_clip.tif')
    if not write_geotiff(clipped, tiff_path):
        return []
    # Debug: print georeference metadata for this tile so we can trace drift
    meta_path = tiff_path.with_suffix('.geo.json')
    if meta_path.exists():
        try:
            with open(meta_path, 'r', encoding='utf-8') as mf:
                meta = json.load(mf)
            print(f'    Tile meta (sidecar): crs={meta.get("crs")} transform={meta.get("geotransform") or meta.get("transform")}')
        except Exception:
            print('    [!] Failed to read sidecar meta')
    else:
        if HAS_RASTERIO:
            try:
                with rasterio.open(tiff_path) as src:
                    print(f'    Tile meta (tiff): crs={src.crs} transform={list(src.transform)} bounds={src.bounds}')
            except Exception:
                print('    [!] Failed to open tile with rasterio for debug')

    print(f'    Running GPU engine (threshold={cfg["threshold"]})...')
    anomalies = run_rust_engine(tiff_path, cfg['threshold'])

    if anomalies and 'error' in anomalies[0]:
        print(f'    [!] Engine: {anomalies[0]["error"][:80]}')
        return []

    print(f'    Anomalies: {len(anomalies)}')

    year = clipped.get('year')
    doy  = clipped.get('doy')
    date_str = doy_to_date(year, doy) if year and doy else ''

    detections = []
    for anom in anomalies:
        if 'error' in anom:
            continue
        lat, lon = pixel_to_latlon(anom['row'], anom['col'], clipped)
        if not (BBOX['lat_min'] <= lat <= BBOX['lat_max'] and
                BBOX['lon_min'] <= lon <= BBOX['lon_max']):
            continue
        utm_e, utm_n = latlon_to_utm16t(lat, lon)
        _, _, utm_zone = latlon_to_utm(lat, lon)
        detections.append({
            'sensor'     : cfg['label'],
            'sensor_key' : sensor_key,
            'mode'       : cfg['mode'],
            'date'       : date_str,
            'year'       : year,
            'doy'        : doy,
            'lat'        : lat,
            'lon'        : lon,
            'utm_zone'   : utm_zone,
            'utm'        : {'easting': utm_e, 'northing': utm_n, 'zone': utm_zone},
            'grid_ref'   : 'WH2K-%04d-%04d' % (int(utm_e/2000), int(utm_n/2000)),
            'row'        : anom['row'],
            'col'        : anom['col'],
            'zscore'     : anom['zscore'],
            'threshold'  : cfg['threshold'],
            'source_file': fname,
            'tiff_path'  : str(tiff_path),
            'in_lake'    : True,
        })

    file_json = OUTPUT_DIR / fname.replace('.h5', '_detections.json')
    with open(file_json, 'w') as fj:
        json.dump({'source': fname, 'sensor': sensor_key, 'date': date_str,
                   'threshold': cfg['threshold'], 'mode': cfg['mode'],
                   'detections': detections}, fj, indent=2)

    return detections

# ── Main ───────────────────────────────────────────────────────────────────────

def run():
    print('=' * 72)
    print('STRAITS/SOUTH FOX -- RUST GPU ENGINE RUNNER')
    print('=' * 72)
    print(f'  Input:  {INPUT_ROOT}')
    print(f'  Output: {OUTPUT_DIR}')
    print(f'  Engine: {GPU_EXE}  (exists={GPU_EXE.exists()})')
    print()

    if not GPU_EXE.exists():
        print('[!] GPU engine not found. Build: cargo build --release --bin cesarops-gpu')
        print('    GeoTIFFs will still be generated for manual runs.')
        print()

    sensor_dirs = {
        'VNP21A1D': INPUT_ROOT / 'viirs_lst',
        'VNP46A1' : INPUT_ROOT / 'viirs_dnb',
    }

    all_files = []
    for sk, sd in sensor_dirs.items():
        if sd.exists():
            for f in sorted(sd.glob('*.h5')):
                all_files.append((f, sk))

    print(f'Files to process: {len(all_files)}')
    print()

    all_detections  = []
    total_processed = 0
    total_failed    = 0

    for i, (h5_path, sensor_key) in enumerate(all_files, 1):
        print(f'[{i}/{len(all_files)}]')
        try:
            dets = process_file(h5_path, sensor_key)
            all_detections.extend(dets)
            total_processed += 1
        except Exception as ex:
            print(f'    [!] Error: {ex}')
            total_failed += 1
        print()

    all_detections.sort(key=lambda x: abs(x.get('zscore', 0)), reverse=True)

    # Auto-cluster detections
    print('Clustering repeated observations...')
    from math import radians, sin, cos, asin
    
    def distance_m(lat1, lon1, lat2, lon2):
        lat1, lon1, lat2, lon2 = map(radians, [lat1, lon1, lat2, lon2])
        dlat = lat2 - lat1
        dlon = lon2 - lon1
        a = sin(dlat/2)**2 + cos(lat1) * cos(lat2) * sin(dlon/2)**2
        return 6371000 * 2 * asin(math.sqrt(a))
    
    clusters = []
    used = set()
    
    for i, det in enumerate(all_detections):
        if i in used:
            continue
        cluster = {'observations': [det], 'indices': [i]}
        for j, other in enumerate(all_detections):
            if j <= i or j in used:
                continue
            dist = distance_m(det['lat'], det['lon'], other['lat'], other['lon'])
            zscore_diff = abs(det['zscore'] - other['zscore'])
            same_mode = (det['mode'] == other['mode'])
            if dist <= 1000 and zscore_diff <= 1.5 and same_mode:
                cluster['observations'].append(other)
                cluster['indices'].append(j)
                used.add(j)
        used.add(i)
        clusters.append(cluster)
    
    clustered = []
    for cluster in clusters:
        obs = cluster['observations']
        n = len(obs)
        avg_lat = sum(o['lat'] for o in obs) / n
        avg_lon = sum(o['lon'] for o in obs) / n
        avg_zscore = sum(o['zscore'] for o in obs) / n
        
        if n == 1:
            confidence = 'LOW'
            color = 'yellow'
        elif n <= 3:
            confidence = 'MEDIUM'
            color = 'orange'
        elif n <= 5:
            confidence = 'HIGH'
            color = 'red'
        else:
            confidence = 'VERY_HIGH'
            color = 'darkred'
        
        mode = obs[0]['mode']
        if mode == 'cold_sink':
            temp_class = 'COLD'
            kml_folder = 'Cold Thermal Signatures'
            color = 'blue' if n == 1 else 'darkblue' if n <= 3 else 'purple'
        else:
            temp_class = 'WARM'
            kml_folder = 'Warm Thermal Signatures'
        
        observation_dates = sorted(set(o['date'] for o in obs))
        zscore_range = [min(o['zscore'] for o in obs), max(o['zscore'] for o in obs)]
        
        clustered.append({
            'lat': round(avg_lat, 6),
            'lon': round(avg_lon, 6),
            'utm': obs[0]['utm'],
            'grid_ref': obs[0]['grid_ref'],
            'temperature_class': temp_class,
            'kml_folder': kml_folder,
            'kml_color': color,
            'observation_count': n,
            'confidence': confidence,
            'avg_zscore': round(avg_zscore, 3),
            'zscore_range': [round(zscore_range[0], 3), round(zscore_range[1], 3)],
            'observation_dates': observation_dates,
            'sensors': sorted(set(o['sensor'] for o in obs)),
            'source_files': [o['source_file'] for o in obs],
            'mode': mode,
            'sensor': obs[0]['sensor'],
            'date': observation_dates[0] if observation_dates else '',
        })
    
    confidence_order = {'VERY_HIGH': 0, 'HIGH': 1, 'MEDIUM': 2, 'LOW': 3}
    clustered.sort(key=lambda x: (confidence_order[x['confidence']], -abs(x['avg_zscore'])))
    
    print(f'  Clustered: {len(all_detections)} -> {len(clustered)} ({round((1-len(clustered)/len(all_detections))*100,1)}% reduction)')

    master = {
        'run_at'          : datetime.now().isoformat(),
        'bbox'            : BBOX,
        'files_processed' : total_processed,
        'files_failed'    : total_failed,
        'total_detections': len(all_detections),
        'clustered_detections': len(clustered),
        'reduction_percent': round((1 - len(clustered)/len(all_detections)) * 100, 1) if all_detections else 0,
        'cold_sink_hits'  : sum(1 for d in all_detections if d.get('mode') == 'cold_sink'),
        'thermal_hits'    : sum(1 for d in all_detections if d.get('mode') == 'thermal_contrast'),
        'confidence_breakdown': {
            'VERY_HIGH': sum(1 for d in clustered if d['confidence'] == 'VERY_HIGH'),
            'HIGH': sum(1 for d in clustered if d['confidence'] == 'HIGH'),
            'MEDIUM': sum(1 for d in clustered if d['confidence'] == 'MEDIUM'),
            'LOW': sum(1 for d in clustered if d['confidence'] == 'LOW'),
        },
        'top_20'          : clustered[:20],
        'all_detections'  : clustered,
        'raw_detections'  : all_detections,
    }

    master_path = OUTPUT_DIR / 'straits_engine_master_report.json'
    with open(master_path, 'w') as f:
        json.dump(master, f, indent=2)

    kml_path = OUTPUT_DIR / 'straits_engine_detections.kml'
    if clustered:
        write_kml(clustered, kml_path)

    print('=' * 72)
    print('ENGINE RUN COMPLETE')
    print('=' * 72)
    print(f'  Files processed : {total_processed}')
    print(f'  Files failed    : {total_failed}')
    print(f'  Raw detections  : {len(all_detections)}')
    print(f'  Clustered pins  : {len(clustered)} ({master["reduction_percent"]}% reduction)')
    print(f'  Cold-sink hits  : {master["cold_sink_hits"]}')
    print(f'  Thermal hits    : {master["thermal_hits"]}')
    print()
    print('  CONFIDENCE BREAKDOWN:')
    for conf in ['VERY_HIGH', 'HIGH', 'MEDIUM', 'LOW']:
        count = master['confidence_breakdown'][conf]
        if count > 0:
            print(f'    {conf}: {count}')
    if clustered:
        print()
        print('  TOP 5 CLUSTERED:')
        for d in clustered[:5]:
            print(f'    [{d["confidence"]}] {d["sensor"]} {d["observation_count"]}x obs  '
                  f'Z={d["avg_zscore"]:.2f}  {d["lat"]},{d["lon"]}')
    print()
    print(f'  Report: {master_path}')
    if clustered:
        print(f'  KML:    {kml_path}')
    print('=' * 72)


if __name__ == '__main__':
    run()
