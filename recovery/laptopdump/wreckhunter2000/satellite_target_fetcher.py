"""
satellite_target_fetcher.py

Automated satellite data puller for Lake Michigan (or any bounding box).
Fetches multi-sensor data from NASA Earthdata and generates Google Earth KML/KMZ
with color-coded confidence pins.

CUDA-ACCELERATED: Uses PyTorch on Quadro M2200 for target detection and ranking.

Sensors:
  - Sentinel-2 L2A (Optical, 10m/20m/60m) via earth-search STAC
  - Sentinel-1 SAR (C-band, VV/VH) via ASF DAAC
  - SWOT Ka-band (SSH, 1cm height) via PO.DAAC
  - Landsat 8/9 TIRS (Thermal, 100m) via LP DAAC
  - ICESat-2 ATL13 (Laser height, 17m footprint) via NSIDC
"""

import json
import math
import os
import sys
import tempfile
import zipfile
from datetime import datetime, timezone
from pathlib import Path
from xml.etree import ElementTree as ET

import requests

# ── CUDA Setup ────────────────────────────────────────────────────────────────

try:
    import torch
    import numpy as np
    
    # Check CUDA availability
    if torch.cuda.is_available():
        DEVICE = torch.device('cuda')
        print(f'[+] CUDA acceleration enabled: {torch.cuda.get_device_name(0)}')
        print(f'    PyTorch {torch.__version__} + CUDA {torch.version.cuda}')
    else:
        DEVICE = torch.device('cpu')
        print('[!] CUDA not available — running on CPU')
        DEVICE = torch.device('cpu')
except ImportError:
    DEVICE = torch.device('cpu')
    print('[!] PyTorch not installed — running on CPU')
    np = None

# ── Configuration ─────────────────────────────────────────────────────────────

# Default: Lake Michigan corridor (Zion/Waukegan to Milwaukee)
DEFAULT_BBOX = {
    'lon_min': -87.15,  # West
    'lat_min': 42.44,   # South
    'lon_max': -87.06,  # East
    'lat_max': 42.49,   # North
}

# Full Lake Michigan bounding box
LAKE_MICHIGAN_BBOX = {
    'lon_min': -87.9,
    'lat_min': 41.5,
    'lon_max': -85.5,
    'lat_max': 46.0,
}

# Earthdata token path
TOKEN_PATHS = [
    Path('c:/Users/thomf/programming/Bagrecovery/sentinel_hunt/earthdata_token.json'),
    Path('c:/Users/thomf/programming/Bagrecovery/erie_remote/erie_remote_data/.earthdata_token'),
    Path.home() / '.netrc',
]

# Output directory
REPO = Path(__file__).resolve().parent
OUTPUT_DIR = REPO / 'outputs' / 'satellite_targets'
OUTPUT_DIR.mkdir(parents=True, exist_ok=True)

# Confidence color codes (Google Earth KML colors: AABBGGRR)
CONFIDENCE_COLORS = {
    'HIGH': 'ff0000ff',      # Red (highest confidence)
    'MEDIUM': 'ff00ffff',    # Yellow
    'LOW': 'ff00ff00',       # Green
    'PENDING': 'ffffff00',   # Cyan (awaiting validation)
}

# Sensor priorities
SENSOR_PRIORITY = {
    'sentinel2': 1,
    'sentinel1': 2,
    'swot': 3,
    'landsat': 4,
    'icesat2': 5,
}

# ── Helpers ───────────────────────────────────────────────────────────────────

def load_earthdata_token() -> str:
    """Load Earthdata token from JSON file."""
    for tp in TOKEN_PATHS:
        if tp.exists():
            try:
                if tp.suffix == '.json':
                    data = json.loads(tp.read_text(encoding='utf-8'))
                    return data.get('earthdata_token', '')
                else:
                    return tp.read_text(encoding='utf-8').strip()
            except Exception:
                continue
    return ''


def bbox_to_string(bbox: dict) -> str:
    """Convert bbox dict to CMR format string: 'lon_min,lat_min,lon_max,lat_max'."""
    return f"{bbox['lon_min']},{bbox['lat_min']},{bbox['lon_max']},{bbox['lat_max']}"


def _haversine_m(lat1, lon1, lat2, lon2) -> float:
    """Calculate distance between two coordinates in meters."""
    R = 6_371_000.0
    phi1, phi2 = math.radians(lat1), math.radians(lat2)
    dphi = math.radians(lat2 - lat1)
    dlam = math.radians(lon2 - lon1)
    a = math.sin(dphi/2)**2 + math.cos(phi1)*math.cos(phi2)*math.sin(dlam/2)**2
    return R * 2 * math.asin(math.sqrt(a))


def compute_confidence_score(hit: dict) -> str:
    """
    Compute confidence level based on available sensor data.
    
    HIGH:   Thermal sink (Z < -2.0) + SAR stability (>0.9) + SWOT height (>1cm)
    MEDIUM: 2+ sensors agree
    LOW:    Single sensor detection
    PENDING: Awaiting validation
    """
    thermal_ok = hit.get('thermal_zscore') is not None and hit.get('thermal_zscore') < -2.0
    sar_ok = hit.get('sar_stability') is not None and hit.get('sar_stability') > 0.9
    swot_ok = hit.get('swot_height_m') is not None and abs(hit.get('swot_height_m', 0)) > 0.01
    
    sensors_agree = sum([thermal_ok, sar_ok, swot_ok])
    
    if sensors_agree == 3:
        return 'HIGH'
    elif sensors_agree == 2:
        return 'MEDIUM'
    elif sensors_agree == 1:
        return 'LOW'
    else:
        return 'PENDING'


# ── NASA CMR Queries ──────────────────────────────────────────────────────────

def query_cmr_granules(
    short_name: str,
    bbox: dict,
    temporal: tuple[str, str],
    token: str = '',
    page_size: int = 100
) -> list[dict]:
    """
    Query NASA CMR for granules matching criteria.
    
    Args:
        short_name: Product short name (e.g., 'SENTINEL-2_L2A')
        bbox: Bounding box dict
        temporal: (start_date, end_date) tuple
        token: Earthdata token for authenticated access
        page_size: Results per page
    
    Returns:
        List of granule metadata dicts
    """
    cmr_base = 'https://cmr.earthdata.nasa.gov/search/granules.json'
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = f'Bearer {token}'
    
    params = {
        'short_name': short_name,
        'temporal': f'{temporal[0]}T00:00:00Z,{temporal[1]}T23:59:59Z',
        'bounding_box': bbox_to_string(bbox),
        'page_size': page_size,
        'sort_key': '-start_date',  # Newest first
    }
    
    try:
        resp = requests.get(cmr_base, params=params, headers=headers, timeout=60)
        resp.raise_for_status()
        entries = resp.json().get('feed', {}).get('entry', [])
        return entries
    except Exception as e:
        print(f'[!] CMR query failed for {short_name}: {e}')
        return []


def fetch_sentinel2_l2a(bbox: dict, date_range: tuple[str, str], token: str = '') -> list[dict]:
    """Fetch Sentinel-2 L2A products via earth-search STAC (no token needed)."""
    stac_search = 'https://earth-search.aws.element84.com/v1/search'
    
    # Parse date range
    start_date = date_range[0].replace('-', '')
    end_date = date_range[1].replace('-', '')
    
    payload = {
        'collections': ['sentinel-2-l2a'],
        'datetime': f'{date_range[0]}T00:00:00Z/{date_range[1]}T23:59:59Z',
        'intersects': {
            'type': 'Polygon',
            'coordinates': [[
                [bbox['lon_min'], bbox['lat_min']],
                [bbox['lon_max'], bbox['lat_min']],
                [bbox['lon_max'], bbox['lat_max']],
                [bbox['lon_min'], bbox['lat_max']],
                [bbox['lon_min'], bbox['lat_min']],
            ]]
        },
        'query': {
            'eo:cloud_cover': {'lte': 20},  # Max 20% cloud cover
        },
        'limit': 50,
    }
    
    try:
        resp = requests.post(stac_search, json=payload, timeout=60)
        resp.raise_for_status()
        features = resp.json().get('features', [])
        
        results = []
        for f in features:
            props = f.get('properties', {})
            results.append({
                'sensor': 'sentinel2',
                'granule_id': f.get('id', ''),
                'scene_date': props.get('datetime', '')[:10],
                'cloud_cover': props.get('eo:cloud_cover', 100),
                'mgrs_tile': props.get('mgrs:grid_square', ''),
                'assets': f.get('assets', {}),
                'bbox': f.get('bbox', []),
            })
        
        return results
    except Exception as e:
        print(f'[!] Sentinel-2 STAC query failed: {e}')
        return []


def fetch_sentinel1_sar(bbox: dict, date_range: tuple[str, str], token: str = '') -> list[dict]:
    """Fetch Sentinel-1 SAR products via ASF DAAC."""
    return query_cmr_granules('SENTINEL-1_SLC', bbox, date_range, token, page_size=50)


def fetch_swot_ssh(bbox: dict, date_range: tuple[str, str], token: str = '') -> list[dict]:
    """Fetch SWOT SSH Expert products via PO.DAAC."""
    return query_cmr_granules('SWOT_L2_LR_SSH_2.0', bbox, date_range, token, page_size=100)


def fetch_landsat_thermal(bbox: dict, date_range: tuple[str, str], token: str = '') -> list[dict]:
    """Fetch Landsat 8/9 TIRS thermal products via LP DAAC."""
    results = []
    for short_name in ['LANDSAT_OT_C2_L2', 'LANDSAT_TM_C2_L2']:
        granules = query_cmr_granules(short_name, bbox, date_range, token, page_size=30)
        for g in granules:
            g['sensor'] = 'landsat'
            results.append(g)
    return results


def fetch_icesat2_atl13(bbox: dict, date_range: tuple[str, str], token: str = '') -> list[dict]:
    """Fetch ICESat-2 ATL13 laser height products via NSIDC."""
    return query_cmr_granules('ATL13', bbox, date_range, token, page_size=50)


# ── CUDA-Accelerated Target Processing ────────────────────────────────────────

def cuda_detect_targets_from_db() -> list[dict]:
    """
    CUDA-accelerated target detection from census database.
    Uses PyTorch tensors on GPU for parallel score computation and ranking.
    """
    if np is None:
        # Fallback to CPU-only
        return detect_targets_from_sentinel2({}, '')
    
    census_db = REPO / 'LAKE_MICHIGAN_CENSUS_2026.db'
    if not census_db.exists():
        return []
    
    import sqlite3
    conn = sqlite3.connect(str(census_db))
    cur = conn.cursor()
    
    # Fetch all anomaly hits
    cur.execute("""
        SELECT lat, lon, concept, score, wreck_score, metric_zscore,
               epoch_date, scene_id, sun_azimuth_deg, sat_zenith_deg,
               thermal_sink_l8, sar_stability_s1, swot_height_anomaly_m
        FROM anomaly_hits
        WHERE score IS NOT NULL
        ORDER BY score DESC
    """)
    
    rows = cur.fetchall()
    conn.close()
    
    if not rows:
        return []
    
    # Convert to numpy arrays for CUDA processing
    lats = np.array([r[0] for r in rows], dtype=np.float32)
    lons = np.array([r[1] for r in rows], dtype=np.float32)
    scores = np.array([r[3] for r in rows], dtype=np.float32)
    wreck_scores = np.array([r[4] for r in rows], dtype=np.float32)
    zscores = np.array([r[5] for r in rows], dtype=np.float32)
    
    # Move to GPU
    scores_tensor = torch.from_numpy(scores).to(DEVICE)
    wreck_scores_tensor = torch.from_numpy(wreck_scores).to(DEVICE)
    zscores_tensor = torch.from_numpy(zscores).to(DEVICE)
    
    # CUDA-accelerated confidence computation
    # Normalize scores to 0-1 range
    max_score = torch.max(scores_tensor)
    normalized_scores = scores_tensor / (max_score + 1e-8)
    
    # Compute confidence based on multiple factors (all on GPU)
    confidence_scores = (
        normalized_scores * 0.4 +  # Original score weight
        (wreck_scores_tensor / 10.0) * 0.3 +  # Wreck score weight
        (torch.clamp(zscores_tensor / 10.0, 0, 1)) * 0.3  # Z-score weight
    )
    
    # Move results back to CPU
    confidence_cpu = confidence_scores.cpu().numpy()
    
    # Build target list
    targets = []
    for i, row in enumerate(rows):
        lat, lon, concept, score, wreck_score, zscore, epoch, scene_id, sun_az, sat_zen, thermal, sar, swot = row
        
        hit_data = {
            'lat': float(lat), 'lon': float(lon), 'concept': concept,
            'score': float(score), 'wreck_score': int(wreck_score),
            'zscore': float(zscore) if zscore else None,
            'epoch_date': epoch, 'scene_id': scene_id,
            'sun_azimuth': float(sun_az) if sun_az else None,
            'sat_zenith': float(sat_zen) if sat_zen else None,
            'thermal_sink_l8': thermal,
            'sar_stability': float(sar) if sar else None,
            'swot_height_m': float(swot) if swot else None,
        }
        
        # Compute confidence level from CUDA score
        conf_score = float(confidence_cpu[i])
        if conf_score > 0.8:
            confidence = 'HIGH'
        elif conf_score > 0.6:
            confidence = 'MEDIUM'
        elif conf_score > 0.4:
            confidence = 'LOW'
        else:
            confidence = 'PENDING'
        
        targets.append({
            'id': f'TGT-{i+1:04d}',
            'lat': float(lat),
            'lon': float(lon),
            'confidence': confidence,
            'score': float(score),
            'concept': concept,
            'sensor_data': hit_data,
            'source_scene': scene_id or 'census_db',
            'cuda_confidence_score': conf_score,
        })
    
    print(f'  CUDA target processing: {len(targets)} targets on {DEVICE}')
    return targets


def cuda_rank_targets(targets: list[dict]) -> list[dict]:
    """
    CUDA-accelerated target ranking.
    Uses GPU parallel sort for large target sets.
    """
    if not targets or np is None:
        # Fallback to CPU
        return rank_targets_cpu(targets)
    
    # Extract scores and confidence for GPU sorting
    confidence_map = {'HIGH': 0, 'MEDIUM': 1, 'LOW': 2, 'PENDING': 3}
    conf_values = np.array([confidence_map.get(t['confidence'], 3) for t in targets], dtype=np.int32)
    score_values = np.array([t['score'] or 0 for t in targets], dtype=np.float32)
    
    # Move to GPU
    conf_tensor = torch.from_numpy(conf_values).to(DEVICE)
    score_tensor = torch.from_numpy(score_values).to(DEVICE)
    
    # Compute composite rank score (lower confidence priority + higher score = better rank)
    # Normalize scores to 0-1
    max_score = torch.max(score_tensor) + 1e-8
    rank_score = conf_tensor.float() - (score_tensor / max_score)
    
    # GPU argsort
    _, sorted_indices = torch.sort(rank_score, descending=False)
    
    # Reorder targets based on GPU sort
    sorted_targets = [targets[idx] for idx in sorted_indices.cpu().numpy()]
    
    # Assign ranks
    for i, t in enumerate(sorted_targets, 1):
        t['rank'] = i
    
    print(f'  CUDA ranking complete: {len(sorted_targets)} targets sorted on {DEVICE}')
    return sorted_targets


def rank_targets_cpu(targets: list[dict]) -> list[dict]:
    """CPU fallback for target ranking."""
    confidence_order = {'HIGH': 0, 'MEDIUM': 1, 'LOW': 2, 'PENDING': 3}
    targets.sort(key=lambda t: (confidence_order.get(t['confidence'], 3), -(t['score'] or 0)))
    for i, t in enumerate(targets, 1):
        t['rank'] = i
    return targets


# ── KML/KMZ Generation ────────────────────────────────────────────────────────

def create_kml_document(targets: list[dict], bbox: dict, metadata: dict) -> str:
    """
    Generate KML document with color-coded pins.
    
    Args:
        targets: List of ranked target dicts
        bbox: Bounding box used for query
        metadata: Run metadata (date, sensor counts, etc.)
    
    Returns:
        KML XML string
    """
    # KML namespace
    ns = {'kml': 'http://www.opengis.net/kml/2.2'}
    
    # Build KML structure
    kml = ET.Element('{http://www.opengis.net/kml/2.2}kml')
    doc = ET.SubElement(kml, 'Document')
    
    # Document metadata
    name = ET.SubElement(doc, 'name')
    name.text = f"Satellite Targets - {datetime.now().strftime('%Y-%m-%d %H:%M')}"
    
    desc = ET.SubElement(doc, 'description')
    desc.text = (
        f"Bounding Box: {bbox['lon_min']:.4f},{bbox['lat_min']:.4f} to "
        f"{bbox['lon_max']:.4f},{bbox['lat_max']:.4f}\\n"
        f"Generated: {metadata['run_at']}\\n"
        f"Total Targets: {len(targets)}\\n"
        f"HIGH: {metadata['high_count']} | MEDIUM: {metadata['medium_count']} | "
        f"LOW: {metadata['low_count']} | PENDING: {metadata['pending_count']}"
    )
    
    # Add styles for each confidence level
    for conf_level, color in CONFIDENCE_COLORS.items():
        style = ET.SubElement(doc, 'Style', id=f'style_{conf_level}')
        icon_style = ET.SubElement(style, 'IconStyle')
        color_elem = ET.SubElement(icon_style, 'color')
        color_elem.text = color
        scale_elem = ET.SubElement(icon_style, 'scale')
        scale_elem.text = '1.2'
        icon = ET.SubElement(icon_style, 'Icon')
        href = ET.SubElement(icon, 'href')
        href.text = 'http://maps.google.com/mapfiles/kml/pushpin/ylw-pushpin.png'
    
    # Add targets as placemarks
    for target in targets:
        placemark = ET.SubElement(doc, 'Placemark')
        
        # Name with rank and confidence
        pm_name = ET.SubElement(placemark, 'name')
        pm_name.text = f"#{target['rank']} [{target['confidence']}]"
        
        # Description with full metadata
        pm_desc = ET.SubElement(placemark, 'description')
        sensor_data = target.get('sensor_data', {})
        desc_text = (
            f"<![CDATA["
            f"<h3>Target {target['id']}</h3>"
            f"<table>"
            f"<tr><td><b>Rank:</b></td><td>#{target['rank']}</td></tr>"
            f"<tr><td><b>Confidence:</b></td><td>{target['confidence']}</td></tr>"
            f"<tr><td><b>Score:</b></td><td>{target['score']}</td></tr>"
            f"<tr><td><b>Concept:</b></td><td>{target['concept']}</td></tr>"
            f"<tr><td><b>Wreck Score:</b></td><td>{sensor_data.get('wreck_score', 'N/A')}</td></tr>"
            f"<tr><td><b>Z-Score:</b></td><td>{sensor_data.get('zscore', 'N/A')}</td></tr>"
            f"<tr><td><b>Epoch:</b></td><td>{sensor_data.get('epoch_date', 'N/A')}</td></tr>"
            f"<tr><td><b>Sun Azimuth:</b></td><td>{sensor_data.get('sun_azimuth', 'N/A')}°</td></tr>"
            f"<tr><td><b>Sat Zenith:</b></td><td>{sensor_data.get('sat_zenith', 'N/A')}°</td></tr>"
            f"</table>"
            f"<p><i>Source: {target['source_scene']}</i></p>"
            f"]]>"
        )
        pm_desc.text = desc_text
        
        # Style reference
        style_url = ET.SubElement(placemark, 'styleUrl')
        style_url.text = f'#style_{target["confidence"]}'
        
        # Point coordinates
        point = ET.SubElement(placemark, 'Point')
        coords = ET.SubElement(point, 'coordinates')
        coords.text = f"{target['lon']},{target['lat']},0"
    
    # Convert to string
    xml_str = ET.tostring(kml, encoding='unicode', xml_declaration=True)
    return xml_str


def save_kml(kml_content: str, output_path: Path) -> Path:
    """Save KML content to file."""
    output_path.write_text(kml_content, encoding='utf-8')
    return output_path


def save_kmz(kml_content: str, output_path: Path) -> Path:
    """Save KML content as KMZ (zipped KML)."""
    kmz_path = output_path.with_suffix('.kmz')
    with zipfile.ZipFile(str(kmz_path), 'w', zipfile.ZIP_DEFLATED) as zf:
        zf.writestr('doc.kml', kml_content.encode('utf-8'))
    return kmz_path


# ── Main Pipeline ─────────────────────────────────────────────────────────────

def run_satellite_target_fetch(
    bbox: dict = None,
    date_range: tuple[str, str] = None,
    output_format: str = 'both',  # 'kml', 'kmz', or 'both'
    token: str = None
) -> dict:
    """
    Main pipeline: Fetch satellite data, detect targets, generate KML/KMZ.
    
    Args:
        bbox: Bounding box dict (default: Lake Michigan corridor)
        date_range: (start_date, end_date) tuple (default: last 90 days)
        output_format: 'kml', 'kmz', or 'both'
        token: Earthdata token (default: load from file)
    
    Returns:
        Summary dict with counts and output paths
    """
    # Defaults
    if bbox is None:
        bbox = DEFAULT_BBOX.copy()
    if date_range is None:
        # Default: last 90 days
        from datetime import timedelta
        end_dt = datetime.now(timezone.utc)
        start_dt = end_dt - timedelta(days=90)
        date_range = (start_dt.strftime('%Y-%m-%d'), end_dt.strftime('%Y-%m-%d'))
    if token is None:
        token = load_earthdata_token()
    
    run_at = datetime.now(timezone.utc).isoformat()
    print(f'[+] Satellite Target Fetcher — {run_at}')
    print(f'[+] Bounding Box: {bbox}')
    print(f'[+] Date Range: {date_range[0]} to {date_range[1]}')
    print(f'[+] Earthdata Token: {"loaded" if token else "NOT FOUND"}')
    print()
    
    # Step 1: Fetch satellite data from multiple sensors
    print('[Step 1] Querying satellite data sources...')
    
    print('  Fetching Sentinel-2 L2A (earth-search STAC)...')
    s2_scenes = fetch_sentinel2_l2a(bbox, date_range, token)
    print(f'    Found {len(s2_scenes)} scenes')
    
    print('  Fetching Sentinel-1 SAR (ASF DAAC)...')
    s1_granules = fetch_sentinel1_sar(bbox, date_range, token)
    print(f'    Found {len(s1_granules)} granules')
    
    print('  Fetching SWOT SSH (PO.DAAC)...')
    swot_granules = fetch_swot_ssh(bbox, date_range, token)
    print(f'    Found {len(swot_granules)} granules')
    
    print('  Fetching Landsat Thermal (LP DAAC)...')
    landsat_granules = fetch_landsat_thermal(bbox, date_range, token)
    print(f'    Found {len(landsat_granules)} granules')
    
    print('  Fetching ICESat-2 ATL13 (NSIDC)...')
    icesat2_granules = fetch_icesat2_atl13(bbox, date_range, token)
    print(f'    Found {len(icesat2_granules)} granules')
    
    print()
    
    # Step 2: Detect targets from satellite data
    print('[Step 2] Detecting anomaly targets...')
    targets = []
    
    # Use existing census results for now
    for scene in s2_scenes[:1]:  # Process first scene
        scene_targets = detect_targets_from_sentinel2(scene, token)
        targets.extend(scene_targets)
    
    # If no scenes found, load from census DB directly
    if not targets:
        print('  No new Sentinel-2 scenes — loading from census database...')
        dummy_scene = {'granule_id': 'census_db'}
        targets = detect_targets_from_sentinel2(dummy_scene, token)
    
    print(f'  Detected {len(targets)} targets')
    print()
    
    # Step 3: Rank targets by confidence
    print('[Step 3] Ranking targets by confidence...')
    ranked_targets = rank_targets(targets)
    
    # Count by confidence
    conf_counts = {'HIGH': 0, 'MEDIUM': 0, 'LOW': 0, 'PENDING': 0}
    for t in ranked_targets:
        conf_counts[t['confidence']] = conf_counts.get(t['confidence'], 0) + 1
    
    print(f'  HIGH: {conf_counts["HIGH"]} | MEDIUM: {conf_counts["MEDIUM"]} | '
          f'LOW: {conf_counts["LOW"]} | PENDING: {conf_counts["PENDING"]}')
    print()
    
    # Step 4: Generate KML/KMZ output
    print('[Step 4] Generating KML/KMZ output...')
    
    metadata = {
        'run_at': run_at,
        'bbox': bbox,
        'date_range': date_range,
        'sensor_counts': {
            'sentinel2': len(s2_scenes),
            'sentinel1': len(s1_granules),
            'swot': len(swot_granules),
            'landsat': len(landsat_granules),
            'icesat2': len(icesat2_granules),
        },
        'high_count': conf_counts['HIGH'],
        'medium_count': conf_counts['MEDIUM'],
        'low_count': conf_counts['LOW'],
        'pending_count': conf_counts['PENDING'],
    }
    
    kml_content = create_kml_document(ranked_targets, bbox, metadata)
    
    timestamp = datetime.now().strftime('%Y%m%d_%H%M%S')
    output_base = OUTPUT_DIR / f'satellite_targets_{timestamp}'
    
    output_files = []
    
    if output_format in ('kml', 'both'):
        kml_path = save_kml(kml_content, output_base.with_suffix('.kml'))
        output_files.append(str(kml_path))
        print(f'  Saved KML: {kml_path}')
    
    if output_format in ('kmz', 'both'):
        kmz_path = save_kmz(kml_content, output_base)
        output_files.append(str(kmz_path))
        print(f'  Saved KMZ: {kmz_path}')
    
    print()
    
    # Step 5: Save JSON summary
    json_path = OUTPUT_DIR / f'satellite_targets_{timestamp}.json'
    with open(json_path, 'w', encoding='utf-8') as f:
        json.dump({
            'run_at': run_at,
            'bbox': bbox,
            'date_range': date_range,
            'sensor_counts': metadata['sensor_counts'],
            'confidence_counts': conf_counts,
            'total_targets': len(targets),
            'top_targets': ranked_targets[:20],  # Top 20
            'output_files': output_files,
        }, f, indent=2, default=str)
    print(f'  Saved JSON summary: {json_path}')
    print()
    
    # Summary
    print('=' * 62)
    print('SATELLITE TARGET FETCH — SUMMARY')
    print('=' * 62)
    print(f'  Bounding Box: {bbox["lon_min"]:.4f},{bbox["lat_min"]:.4f} to '
          f'{bbox["lon_max"]:.4f},{bbox["lat_max"]:.4f}')
    print(f'  Date Range: {date_range[0]} to {date_range[1]}')
    print(f'  Sensors Queried: 5 (S2, S1, SWOT, L8/9, ICESat-2)')
    print(f'  Total Targets: {len(targets)}')
    print(f'  HIGH Confidence: {conf_counts["HIGH"]}')
    print(f'  MEDIUM Confidence: {conf_counts["MEDIUM"]}')
    print(f'  LOW Confidence: {conf_counts["LOW"]}')
    print(f'  PENDING: {conf_counts["PENDING"]}')
    print(f'  Output Files:')
    for f in output_files:
        print(f'    - {f}')
    print('=' * 62)
    
    return {
        'run_at': run_at,
        'bbox': bbox,
        'date_range': date_range,
        'total_targets': len(targets),
        'confidence_counts': conf_counts,
        'output_files': output_files,
    }


# ── CLI Interface ─────────────────────────────────────────────────────────────

if __name__ == '__main__':
    import argparse
    
    parser = argparse.ArgumentParser(
        description='Satellite Target Fetcher for Lake Michigan (or custom bbox)'
    )
    parser.add_argument(
        '--bbox',
        type=str,
        default='lake_michigan',
        help='Bounding box: "lake_michigan", "corridor", or "lon_min,lat_min,lon_max,lat_max"'
    )
    parser.add_argument(
        '--start-date',
        type=str,
        default=None,
        help='Start date (YYYY-MM-DD), default: 90 days ago'
    )
    parser.add_argument(
        '--end-date',
        type=str,
        default=None,
        help='End date (YYYY-MM-DD), default: today'
    )
    parser.add_argument(
        '--output',
        type=str,
        default='both',
        choices=['kml', 'kmz', 'both'],
        help='Output format: kml, kmz, or both (default: both)'
    )
    
    args = parser.parse_args()
    
    # Parse bounding box
    if args.bbox == 'lake_michigan':
        bbox = LAKE_MICHIGAN_BBOX
    elif args.bbox == 'corridor':
        bbox = DEFAULT_BBOX
    else:
        try:
            # Strip any quotes and whitespace
            bbox_str = args.bbox.strip().strip('"').strip("'")
            parts = bbox_str.split(',')
            if len(parts) != 4:
                raise ValueError(f"Expected 4 values, got {len(parts)}")
            bbox = {
                'lon_min': float(parts[0].strip()),
                'lat_min': float(parts[1].strip()),
                'lon_max': float(parts[2].strip()),
                'lat_max': float(parts[3].strip()),
            }
        except Exception as e:
            print(f'[!] Invalid bbox format: {e}')
            print('    Use: lon_min,lat_min,lon_max,lat_max')
            print('    Example: -87.15,42.44,-87.06,42.49')
            print('    Or use presets: corridor, lake_michigan')
            sys.exit(1)
    
    # Parse date range
    date_range = (args.start_date, args.end_date) if args.start_date and args.end_date else None
    
    # Run pipeline
    result = run_satellite_target_fetch(
        bbox=bbox,
        date_range=date_range,
        output_format=args.output,
    )
    
    print()
    print('[+] Done. Open the KML/KMZ file in Google Earth to view targets.')
