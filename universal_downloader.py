#!/usr/bin/env python3
"""
CESAROPS Universal Satellite Data Downloader

Pulls satellite imagery from all configured sources:
  - ASF HyP3 (Sentinel-1 SAR RTC/InSAR — free, Earthdata auth)
  - Copernicus Data Space (Sentinel-1 SLC, Sentinel-2 MSI — free)
  - NASA PO.DAAC (SWOT SSH, ICESat-2 ATL13 — Earthdata auth)
  - USGS Earth Explorer (Landsat 8/9 — API key)
  - NASA HLS (Harmonized Landsat Sentinel-2 — Earthdata auth)

Usage:
    python universal_downloader.py --area "straits_of_mackinac" --dates 2024-06-01 2024-09-30 --sensors sar,optical
    python universal_downloader.py --bbox 45.8,-84.8,46.1,-84.4 --dates 2013-07-01 2026-04-01 --sensors all
    python universal_downloader.py --list-sources
    python universal_downloader.py --dry-run --area "straits_of_mackinac" --dates 2025-01-01 2025-12-31
"""

import argparse
import json
import os
import subprocess
import sys
import time
import hashlib
import re
from pathlib import Path
from datetime import datetime, timedelta
from typing import Dict, List, Optional
from urllib.parse import urlencode, quote, urljoin, urlparse

import requests
from requests.auth import HTTPBasicAuth

# ── Windows console encoding ────────────────────────────────────────────────
if sys.platform == 'win32':
    sys.stdout.reconfigure(encoding='utf-8')
    sys.stderr.reconfigure(encoding='utf-8')

# ── Config loading ──────────────────────────────────────────────────────────

REPO = Path(__file__).resolve().parent

_ENV_KEYS = (
    'EARTHDATA_TOKEN', 'EARTHDATA_USERNAME', 'EARTHDATA_PASSWORD',
    'NASA_EARTHDATA_TOKEN', 'COPERNICUS_USER', 'COPERNICUS_PASS',
    'USGS_API_KEY', 'USGS_M2M_TOKEN', 'USGS_M2M_API_KEY', 'USGS_M2M_USERNAME',
    'ASF_USERNAME', 'ASF_PASSWORD', 'ASF_TOKEN', 'ASF_API_TOKEN',
    'FEDEO_BASE_URL', 'FEDEO_API_URL', 'FEDEO_STAC_URL',
    'FEDEO_USERNAME', 'FEDEO_PASSWORD',
    'AWS_ACCESS_KEY_ID', 'AWS_SECRET_ACCESS_KEY', 'AWS_SESSION_TOKEN',
    'AWS_DEFAULT_REGION',
)


def load_env(path: Path) -> Dict[str, str]:
    env = {}
    if path.exists():
        for line in path.read_text(encoding='utf-8').splitlines():
            line = line.strip()
            if not line or line.startswith('#') or '=' not in line:
                continue
            key, _, val = line.partition('=')
            env[key.strip()] = val.strip()
    return env


def _parse_shell_exports(path: Path) -> Dict[str, str]:
    """Parse KEY=VALUE lines from credentials.sh-style bash files."""
    env: Dict[str, str] = {}
    if not path.exists():
        return env
    for line in path.read_text(encoding='utf-8').splitlines():
        line = line.strip()
        if not line or line.startswith('#') or '=' not in line:
            continue
        key, _, val = line.partition('=')
        key = key.strip()
        val = val.strip().strip('"').strip("'")
        # Skip unresolved shell references from sourced scripts (e.g. "$USGS_M2M_TOKEN").
        if val.startswith('$') or '${' in val or '$(' in val:
            continue
        if key in _ENV_KEYS and val:
            env[key] = val
    if not env.get('USGS_M2M_TOKEN'):
        text = path.read_text(encoding='utf-8')
        match = re.search(r'USGS_M2M_TOKEN.*==\s*"([^"]+)"', text)
        if match:
            token = match.group(1).strip().strip('"').strip("'")
            if token and token != 'PASTE_USGS_M2M_API_KEY_HERE':
                env['USGS_M2M_TOKEN'] = token
    return env


def _merge_missing(dst: Dict[str, str], src: Dict[str, str]) -> None:
    for k, v in src.items():
        if v and not dst.get(k):
            dst[k] = v


def _load_earthdata_token_json(path: Path) -> str:
    if not path.exists():
        return ''
    try:
        data = json.loads(path.read_text(encoding='utf-8'))
        if isinstance(data, dict):
            return str(data.get('earthdata_token', '') or '').strip()
        return path.read_text(encoding='utf-8').strip()
    except Exception:
        return ''


def _load_bootstrap_script_env() -> Dict[str, str]:
    """Pull EARTHDATA_* from repo bootstrap scripts when .env is missing."""
    env: Dict[str, str] = {}
    scripts = [
        REPO / 'backup' / 'wreckhunter2000-1' / 'scripts' / 'setup_h97_xeon.sh',
        REPO / 'backup' / 'wreckhunter2000-1' / 'bootstrap_xenon.sh',
    ]
    pat = re.compile(
        r'^(EARTHDATA_(?:TOKEN|USERNAME|PASSWORD)|COPERNICUS_(?:USER|PASS)|USGS_API_KEY)=(.+)$'
    )
    for script in scripts:
        if not script.exists():
            continue
        for line in script.read_text(encoding='utf-8').splitlines():
            m = pat.match(line.strip())
            if m and m.group(2).strip() and m.group(1) not in env:
                env[m.group(1)] = m.group(2).strip()
    return env


def _load_credentials_sh_runtime_env(path: Path) -> Dict[str, str]:
    """Source credentials.sh in bash and capture resolved env values for known keys."""
    env: Dict[str, str] = {}
    if not path.exists():
        return env
    cmd = f'set -a; source "{path}" >/dev/null 2>&1; set +a; env'
    try:
        proc = subprocess.run(
            ['bash', '-lc', cmd],
            check=True,
            capture_output=True,
            text=True,
            timeout=20,
        )
    except Exception:
        return env
    for line in proc.stdout.splitlines():
        if '=' not in line:
            continue
        key, _, val = line.partition('=')
        if key in _ENV_KEYS and val:
            env[key] = val
    return env


def bootstrap_credentials() -> Dict[str, str]:
    """Load satellite credentials from .env, credentials.sh, token JSON, bootstrap scripts."""
    merged: Dict[str, str] = {}

    for path in (
        REPO / 'scripts' / 'credentials.sh',
        REPO / 'backup' / 'wreckhunter2000-1' / 'scripts' / 'credentials.sh',
    ):
        _merge_missing(merged, _load_credentials_sh_runtime_env(path))

    for path in (
        Path.home() / '.ssh' / 'credentials',
        Path.home() / '.ssh' / 'credentials.ssh',
        Path('/mnt/data-external/cesarops/repo/.env'),
        Path('/data/cesarops/repo/.env'),
        REPO / '.env',
    ):
        _merge_missing(merged, load_env(path))

    for path in (
        REPO / 'scripts' / 'credentials.sh',
        REPO / 'backup' / 'wreckhunter2000-1' / 'scripts' / 'credentials.sh',
    ):
        _merge_missing(merged, _parse_shell_exports(path))

    for path in (
        REPO / 'backup' / 'deploy' / 'detection' / 'sentinel_hunt' / 'earthdata_token.json',
        REPO / 'backup' / 'wreckhunter2000-1' / 'sentinel_hunt_src' / 'earthdata_token.json',
        REPO / 'pipelines' / 'erie_remote' / 'erie_remote_data' / '.earthdata_token',
    ):
        tok = _load_earthdata_token_json(path)
        if tok and not merged.get('EARTHDATA_TOKEN'):
            merged['EARTHDATA_TOKEN'] = tok

    _merge_missing(merged, _load_bootstrap_script_env())

    # Alias used by nasa_earthdata_client.py
    if merged.get('NASA_EARTHDATA_TOKEN') and not merged.get('EARTHDATA_TOKEN'):
        merged['EARTHDATA_TOKEN'] = merged['NASA_EARTHDATA_TOKEN']
    elif merged.get('EARTHDATA_TOKEN') and not merged.get('NASA_EARTHDATA_TOKEN'):
        merged['NASA_EARTHDATA_TOKEN'] = merged['EARTHDATA_TOKEN']

    if merged.get('USGS_M2M_TOKEN') and not merged.get('USGS_API_KEY'):
        merged['USGS_API_KEY'] = merged['USGS_M2M_TOKEN']
    elif merged.get('USGS_API_KEY') and not merged.get('USGS_M2M_TOKEN'):
        merged['USGS_M2M_TOKEN'] = merged['USGS_API_KEY']
    if merged.get('USGS_API_KEY') and not merged.get('USGS_M2M_API_KEY'):
        merged['USGS_M2M_API_KEY'] = merged['USGS_API_KEY']

    if merged.get('EARTHDATA_TOKEN') and not merged.get('ASF_TOKEN'):
        merged['ASF_TOKEN'] = merged['EARTHDATA_TOKEN']
    if merged.get('ASF_TOKEN') and not merged.get('ASF_API_TOKEN'):
        merged['ASF_API_TOKEN'] = merged['ASF_TOKEN']

    for k, v in merged.items():
        if k in _ENV_KEYS and v and k not in os.environ:
            os.environ[k] = v

    return merged


_dotenv = bootstrap_credentials()

def cfg(key: str, default: str = "") -> str:
    # Support common alias keys across .env / credentials.sh variants.
    aliases = {
        "COPERNICUS_USERNAME": ("COPERNICUS_USER",),
        "COPERNICUS_PASSWORD": ("COPERNICUS_PASS",),
        "COPERNICUS_USER": ("COPERNICUS_USERNAME",),
        "COPERNICUS_PASS": ("COPERNICUS_PASSWORD",),
        "USGS_API_KEY": ("USGS_M2M_TOKEN", "USGS_M2M_API_KEY"),
        "USGS_M2M_TOKEN": ("USGS_API_KEY", "USGS_M2M_API_KEY"),
        "USGS_M2M_API_KEY": ("USGS_API_KEY", "USGS_M2M_TOKEN"),
        "ASF_TOKEN": ("ASF_API_TOKEN", "EARTHDATA_TOKEN"),
        "ASF_API_TOKEN": ("ASF_TOKEN", "EARTHDATA_TOKEN"),
    }
    val = os.environ.get(key, _dotenv.get(key))
    if val:
        return val
    for alt in aliases.get(key, ()):
        alt_val = os.environ.get(alt, _dotenv.get(alt))
        if alt_val:
            return alt_val
    return default

# ── Area presets ────────────────────────────────────────────────────────────

AREAS = {
    'straits_of_mackinac': {
        'bbox': [45.80, -84.80, 46.10, -84.40],
        'label': 'Straits of Mackinac',
    },
    'fox_islands': {
        'bbox': [45.80, -84.60, 46.00, -84.40],
        'label': 'Fox Islands',
    },
    'beaver_islands': {
        'bbox': [45.60, -85.60, 45.80, -85.40],
        'label': 'Beaver Islands',
    },
    'lake_michigan_south': {
        'bbox': [42.30, -88.50, 43.20, -87.40],
        'label': 'Lake Michigan South',
    },
    'lake_michigan_north': {
        'bbox': [43.20, -87.50, 45.00, -86.00],
        'label': 'Lake Michigan North',
    },
    'lake_huron_north': {
        'bbox': [44.50, -83.50, 46.00, -81.50],
        'label': 'Lake Huron North',
    },
    'lake_superior': {
        'bbox': [46.50, -91.00, 48.00, -84.50],
        'label': 'Lake Superior',
    },
    'lake_erie': {
        'bbox': [41.30, -83.50, 42.50, -78.80],
        'label': 'Lake Erie',
    },
    'lake_ontario': {
        'bbox': [43.20, -77.50, 44.20, -76.00],
        'label': 'Lake Ontario',
    },
}


# ── Helper: Earthdata auth session ─────────────────────────────────────────

def _earthdata_token_valid(token: str) -> bool:
    """Return True if JWT Earthdata token exists and is not expired."""
    if not token or token.count('.') < 2:
        return False
    try:
        import base64
        payload = token.split('.')[1] + '=='
        data = json.loads(base64.urlsafe_b64decode(payload))
        return int(data.get('exp', 0)) > int(time.time())
    except Exception:
        return False


def earthdata_session() -> requests.Session:
    """Session for Earthdata-protected downloads (URS redirect flow)."""
    s = requests.Session()
    token = cfg('EARTHDATA_TOKEN') or cfg('NASA_EARTHDATA_TOKEN')
    if token and _earthdata_token_valid(token):
        s.headers.update({'Authorization': f'Bearer {token}'})
    s.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})
    return s


def cmr_session() -> requests.Session:
    """Public CMR granule search — no auth (Basic/Bearer breaks anonymous search)."""
    s = requests.Session()
    s.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})
    return s


class CMRSTACClient:
    """Small helper around CMR-STAC provider endpoints."""

    ROOT = 'https://cmr.earthdata.nasa.gov/stac'

    def __init__(self, provider: str = 'LPCLOUD'):
        self.provider = provider
        self.base = f'{self.ROOT}/{provider}'
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    def search_items(self, collections: List[str], bbox: List[float], start: str,
                     end: str, limit: int = 20) -> List[Dict]:
        payload = {
            'collections': collections,
            'bbox': [bbox[1], bbox[0], bbox[3], bbox[2]],
            'datetime': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'limit': limit,
        }
        resp = self.session.post(f'{self.base}/search', json=payload, timeout=60)
        if resp.status_code != 200:
            return []
        return resp.json().get('features', [])

    def search_collections(self, query: str, limit: int = 30) -> List[Dict]:
        resp = self.session.get(f'{self.base}/collections', params={'q': query, 'limit': limit}, timeout=60)
        if resp.status_code != 200:
            return []
        return resp.json().get('collections', [])

    @staticmethod
    def pick_asset_hrefs(assets: Dict[str, Dict], exts: tuple = ('.tif', '.hdf', '.h5', '.nc')) -> List[str]:
        hrefs: List[str] = []
        for v in assets.values():
            if not isinstance(v, dict):
                continue
            href = (v.get('href') or '').strip()
            if href.startswith('https://') and href.lower().endswith(exts):
                hrefs.append(href)
        return hrefs


_ea_session = None


def earthdata_authenticated_get(url: str, session: requests.Session,
                                timeout: int = 600) -> Optional[requests.Response]:
    """GET protected Earthdata/PO.DAAC URLs via URS OAuth redirect chain.

    Uses a clean session (no preemptive auth headers) so that ASF/LP-DAAC servers
    complete the OAuth handshake without being confused by a Bearer token.  Basic
    credentials are injected *only* when the redirect chain reaches URS itself.
    """
    eu = cfg('EARTHDATA_USERNAME', '')
    ep = cfg('EARTHDATA_PASSWORD', '')

    # --- earthaccess fast path (best option when the library is available) ---
    global _ea_session
    if _ea_session is None and eu and ep:
        try:
            import earthaccess as _ea
            _ea.login(strategy='environment')
            if hasattr(_ea, 'get_requests_https_session'):
                _ea_session = _ea.get_requests_https_session()
            else:
                auth = _ea.login(strategy='environment')
                if hasattr(auth, 'get_session'):
                    _ea_session = auth.get_session()
        except Exception:
            pass

    if _ea_session is not None:
        try:
            return _ea_session.get(url, stream=True, timeout=timeout)
        except Exception:
            pass

    # --- URS OAuth redirect-chain walk ---
    # A clean session with NO Authorization header so that DAAC servers (ASF,
    # LP-DAAC, …) forward us to URS rather than rejecting a Bearer token.
    # Cookies accumulated by this session carry the OAuth state through all hops.
    chain = requests.Session()
    chain.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    import base64 as _b64
    _basic_hdr = (
        'Basic ' + _b64.b64encode(f'{eu}:{ep}'.encode()).decode()
        if eu and ep else None
    )

    current_url = url
    for _ in range(16):
        # S3 pre-signed URLs: download anonymously, no redirect needed
        if 's3.amazonaws.com' in current_url or 'X-Amz-Signature' in current_url:
            return requests.get(current_url, stream=True, timeout=timeout,
                                allow_redirects=True)

        # Inject Basic auth only when hitting URS OAuth/login endpoints
        req_headers = {}
        if 'urs.earthdata.nasa.gov' in current_url and _basic_hdr:
            req_headers['Authorization'] = _basic_hdr

        resp = chain.get(current_url, headers=req_headers,
                         allow_redirects=False, timeout=timeout, stream=True)

        if resp.status_code in (301, 302, 303, 307, 308):
            loc = resp.headers.get('Location', '')
            resp.close()
            if not loc:
                return None
            current_url = urljoin(current_url, loc)
            continue

        return resp

    return None


# ── Source 1: ASF HyP3 (Sentinel-1 SAR) ────────────────────────────────────

class ASFDownloader:
    """Download Sentinel-1 SAR via ASF API.

    Two modes:
      - Direct SLC download (raw granules for local processing)
      - Submit HyP3 job for on-demand RTC processing (cloud, free)
    """

    ASF_SEARCH = 'https://api.daac.asf.alaska.edu/services/search/param'
    HYP3_API = 'https://hyp3-api.asf.alaska.edu'

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'sar')
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

        # Set Earthdata JWT token for HyP3 authentication
        ed_token = cfg('EARTHDATA_TOKEN', '')
        if ed_token:
            self.session.headers.update({'Authorization': f'Bearer {ed_token}'})

    def search_granules(self, bbox: List[float], start: str, end: str,
                        platform: str = 'Sentinel-1A,Sentinel-1B,Sentinel-1C',
                        max_results: int = 50) -> List[Dict]:
        """Search for Sentinel-1 SLC granules in area/time via ASF API."""
        center_lon = (bbox[1] + bbox[3]) / 2
        center_lat = (bbox[0] + bbox[2]) / 2
        params = {
            'platform': platform,
            'processingLevel': 'SLC',
            'intersectsWith': f'POINT({center_lon} {center_lat})',
            'start': f'{start}T00:00:00UTC',
            'end': f'{end}T23:59:59UTC',
            'maxResults': max_results,
            'output': 'jsonlite',  # JSON instead of Metalink XML
        }
        resp = self.session.get(self.ASF_SEARCH, params=params)
        resp.raise_for_status()

        # Parse JSONLite — list of granule dicts
        data = resp.json()
        if not isinstance(data, list):
            data = data.get('results', [])
        granules = []
        for item in data:
            name = item.get('granuleName', item.get('name', ''))
            # Strip .zip extension — HyP3 wants bare granule names
            base_name = name.replace('.zip', '') if name.endswith('.zip') else name
            download_href = (
                item.get('downloadUrl')
                or item.get('url')
                or item.get('fileUrl')
                or ''
            )
            granules.append({
                'id': base_name,
                'title': base_name,
                'href': download_href,
                'start_time': item.get('startTime', ''),
                'polarization': item.get('polarization', 'VV+VH'),
            })
            if len(granules) >= max_results:
                break
        return granules

    def submit_rtc_job(self, granule_name: str, dem: str = 'GLO30',
                       scale: str = 'DECIBEL') -> str:
        """Submit a HyP3 RTC processing job. Returns job_id."""
        # HyP3 API requires a 'jobs' array
        payload = {
            'jobs': [{
                'job_type': 'RTC_GAMMA',
                'job_parameters': {
                    'granules': [granule_name],
                },
            }]
        }
        if self.dry_run:
            print(f"  [DRY RUN] Would submit RTC job for: {granule_name}")
            return 'dry-run-job-id'

        resp = self.session.post(f'{self.HYP3_API}/jobs', json=payload)
        resp.raise_for_status()
        data = resp.json()
        job = data.get('jobs', [{}])[0]
        job_id = job.get('job_id', '?')
        print(f"  HyP3 job submitted: {job_id}")
        print(f"    Granule: {granule_name}")
        print(f"    Status: {job.get('status_code', 'PENDING')}")
        print(f"    Credits: {job.get('credit_cost', '?')}")
        return job_id

    def wait_for_job(self, job_id: str, poll_interval: int = 120,
                     max_wait: int = 36000) -> Optional[str]:
        """Poll HyP3 job until SUCCEEDED/FAILED. Returns download URL or None."""
        if self.dry_run:
            return None

        deadline = time.time() + max_wait
        while time.time() < deadline:
            resp = self.session.get(f'{self.HYP3_API}/jobs/{job_id}')
            resp.raise_for_status()
            job = resp.json()
            # HyP3 returns a dict with 'jobs' key or a single job dict
            if isinstance(job, dict) and 'jobs' in job:
                job = job['jobs'][0]
            status = job.get('status_code', 'UNKNOWN')
            print(f"  Job {job_id}: {status}")

            if status == 'SUCCEEDED':
                # HyP3 returns files list, not links
                for f in job.get('files', []):
                    if isinstance(f, dict) and 'url' in f:
                        return f['url']
                # Fallback: browse_images or thumbnail_images are not what we want
                return None
            elif status in ('FAILED', 'RUNNING_EXPIRED'):
                print(f"  Job failed: {job.get('message', 'unknown error')}")
                return None

            time.sleep(poll_interval)

        print(f"  Job timed out after {max_wait}s")
        return None

    def download_file(self, url: str, dest: Path) -> bool:
        """Download a file with resume support."""
        if self.dry_run:
            print(f"  [DRY RUN] Would download: {url}")
            return True

        dest.parent.mkdir(parents=True, exist_ok=True)
        resp = earthdata_authenticated_get(url, self.session, timeout=600)
        if resp is None:
            print("  ⚠ ASF download failed: no response")
            return False
        if resp.status_code != 200:
            print(f"  ⚠ ASF download failed: {resp.status_code}")
            return False
        with open(dest, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=1 << 20):
                f.write(chunk)
        print(f"  Downloaded: {dest.name} ({dest.stat().st_size / 1e6:.1f} MB)")
        return True

    def run(self, bbox: List[float], start: str, end: str,
            max_granules: int = 20, process_rtc: bool = True, **_) -> List[Path]:
        """Full pipeline: search → submit RTC → wait → download."""
        print(f"\n{'='*60}")
        print(f"ASF HyP3 — Sentinel-1 SAR")
        print(f"  BBOX: {bbox}")
        print(f"  Range: {start} to {end}")
        print(f"{'='*60}")

        granules = self.search_granules(bbox, start, end, max_results=max_granules)
        if not granules:
            print("  No granules found.")
            return []

        print(f"  Found {len(granules)} granules")

        downloaded = []
        for i, g in enumerate(granules):
            print(f"\n[{i+1}/{len(granules)}] {g['title']}")

            if process_rtc:
                job_id = self.submit_rtc_job(g['title'])
                url = self.wait_for_job(job_id)
                if url:
                    dest = self.output_dir / f"rtc_{g['title']}.tif"
                    if self.download_file(url, dest):
                        downloaded.append(dest)
            else:
                # Download raw SLC
                dest = self.output_dir / f"{g['title']}.zip"
                if self.download_file(g['href'], dest):
                    downloaded.append(dest)

        print(f"\n  Total downloaded: {len(downloaded)} files")
        return downloaded


# (CopernicusDownloader removed — dead 403 API, replaced by AWSSentinel2Downloader/FEDEODownloader)

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'copernicus')
        self.session = requests.Session()
        self._token = None
        self._token_expiry = 0

    def _get_token(self) -> Optional[str]:
        user = cfg('COPERNICUS_USERNAME')
        passwd = cfg('COPERNICUS_PASSWORD')
        if not user or not passwd:
            print("  ⚠ COPERNICUS_USERNAME/PASSWORD not set in .env")
            return None

        if self._token and time.time() < self._token_expiry:
            return self._token

        resp = requests.post(self.AUTH_URL, data={
            'client_id': 'cdse-public',
            'username': user,
            'password': passwd,
            'grant_type': 'password',
        })
        if resp.status_code != 200:
            print(f"  ⚠ Copernicus auth failed: {resp.status_code}")
            return None

        data = resp.json()
        self._token = data['access_token']
        self._token_expiry = time.time() + data.get('expires_in', 3500) - 60
        self.session.headers.update({'Authorization': f'Bearer {self._token}'})
        return self._token

    def search(self, bbox: List[float], start: str, end: str,
               product_type: str = 'S2MSI2A',  # Sentinel-2 L2A
               cloud_cover: float = 20.0,
               max_results: int = 50) -> List[Dict]:
        """Search for products in area/time."""
        token = self._get_token()
        if not token:
            return []

        # Sentinel-2 uses $search with OData
        if product_type.startswith('S2'):
            collection = 'SENTINEL-2'
            params = {
                '$filter': (
                    f"Collection/Name eq '{collection}' and "
                    f"OData.CSC.Intersects(area=geography'SRID=4326;POLYGON(("
                    f"{bbox[1]} {bbox[0]}, {bbox[3]} {bbox[0]}, "
                    f"{bbox[3]} {bbox[2]}, {bbox[1]} {bbox[2]}, "
                    f"{bbox[1]} {bbox[0]}))') and "
                    f"Attributes/OData.CSC.StringAttribute/any(att:att/Name eq 'productType' and att/OData.CSC.StringAttribute/Value eq '{product_type}') and "
                    f"ContentDate/Start gt {start}T00:00:00.000Z and "
                    f"ContentDate/Start lt {end}T23:59:59.999Z"
                ),
                '$top': max_results,
            }
        else:
            # Sentinel-1
            collection = 'SENTINEL-1'
            params = {
                '$filter': (
                    f"Collection/Name eq '{collection}' and "
                    f"OData.CSC.Intersects(area=geography'SRID=4326;POLYGON(("
                    f"{bbox[1]} {bbox[0]}, {bbox[3]} {bbox[0]}, "
                    f"{bbox[3]} {bbox[2]}, {bbox[1]} {bbox[2]}, "
                    f"{bbox[1]} {bbox[0]}))') and "
                    f"ContentDate/Start gt {start}T00:00:00.000Z and "
                    f"ContentDate/Start lt {end}T23:59:59.999Z"
                ),
                '$top': max_results,
            }

        resp = self.session.get(self.API_BASE, params=params)
        if resp.status_code != 200:
            print(f"  ⚠ Search failed: {resp.status_code} {resp.text[:200]}")
            return []

        data = resp.json()
        results = []
        for entry in data.get('value', []):
            results.append({
                'id': entry.get('Id', ''),
                'name': entry.get('Name', ''),
                'size': entry.get('ContentLength', 0),
                'date': entry.get('ContentDate', {}).get('Start', ''),
                'download_url': f"{self.API_BASE}({entry.get('Id', '')})/$value",
            })
        return results

    def download(self, product: Dict, dest_dir: Optional[Path] = None) -> Optional[Path]:
        """Download a product to dest_dir."""
        if not self._get_token():
            return None

        dest = (dest_dir or self.output_dir) / f"{product['name']}.zip"
        if self.dry_run:
            print(f"  [DRY RUN] Would download: {product['name']} ({product['size']/1e6:.0f} MB)")
            return dest

        dest.parent.mkdir(parents=True, exist_ok=True)
        url = product['download_url']
        resp = self.session.get(url, stream=True, timeout=3600)
        if resp.status_code != 200:
            print(f"  ⚠ Download failed: {resp.status_code}")
            return None

        with open(dest, 'wb') as f:
            for chunk in resp.iter_content(chunk_size=1 << 20):
                f.write(chunk)
        print(f"  Downloaded: {dest.name} ({dest.stat().st_size / 1e6:.1f} MB)")
        return dest

    def run(self, bbox: List[float], start: str, end: str,
            product_type: str = 'S2MSI2A', max_results: int = 20) -> List[Path]:
        """Search and download products."""
        print(f"\n{'='*60}")
        print(f"Copernicus Data Space — {product_type}")
        print(f"  BBOX: {bbox}")
        print(f"  Range: {start} to {end}")
        print(f"{'='*60}")

        products = self.search(bbox, start, end, product_type, max_results=max_results)
        if not products:
            print("  No products found.")
            return []

        print(f"  Found {len(products)} products")
        downloaded = []
        for i, p in enumerate(products):
            print(f"\n[{i+1}/{len(products)}] {p['name']}")
            result = self.download(p)
            if result:
                downloaded.append(result)

        print(f"\n  Total downloaded: {len(downloaded)} files")
        return downloaded


# ── Source 3: NASA PO.DAAC (SWOT / ICESat-2) ──────────────────────────────

class PODAACDownloader:
    """Download SWOT SSH and ICESat-2 ATL13 from NASA PO.DAAC."""

    CMR_URL = 'https://cmr.earthdata.nasa.gov/search/granules.json'

    COLLECTIONS = {
        # CMR short names as of 2024+ (version field omitted — see search())
        'swot': {
            'short_name': 'SWOT_L2_LR_SSH_EXPERT_2.0',
        },
        'icesat2': {
            'short_name': 'ATL13',
        },
    }

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'podaac')
        self.session = earthdata_session()
        self.cmr = cmr_session()

    def search(self, bbox: List[float], start: str, end: str,
               dataset: str = 'swot', max_results: int = 30) -> List[Dict]:
        """Search CMR for granules."""
        coll = self.COLLECTIONS.get(dataset)
        if not coll:
            print(f"  Unknown dataset: {dataset}")
            return []

        params = {
            'short_name': coll['short_name'],
            'bounding_box': f'{bbox[1]},{bbox[0]},{bbox[3]},{bbox[2]}',
            'temporal': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'page_size': max_results,
        }
        if coll.get('version'):
            params['version'] = coll['version']
        resp = self.cmr.get(self.CMR_URL, params=params)
        if resp.status_code != 200:
            print(f"  ⚠ CMR search failed: {resp.status_code}")
            return []

        data = resp.json()
        granules = []
        for entry in data.get('feed', {}).get('entry', []):
            href = None
            for link in entry.get('links', []):
                rel = link.get('rel', '')
                candidate = link.get('href', '')
                if not candidate.startswith('https://'):
                    continue
                if not candidate.endswith(('.nc', '.h5', '.he5')):
                    continue
                if 'data#' in rel:
                    href = candidate
                    break
                if href is None:
                    href = candidate
            if href:
                granules.append({
                    'id': entry.get('id', ''),
                    'title': entry.get('title', ''),
                    'href': href,
                    'time_start': entry.get('time_start', ''),
                    'dataset': dataset,
                })
        return granules

    def run(self, bbox: List[float], start: str, end: str,
            datasets: List[str] = None, max_results: int = 20) -> List[Path]:
        """Search and download SWOT/ICESat-2 granules."""
        if datasets is None:
            datasets = ['swot', 'icesat2']

        all_downloaded = []
        for ds in datasets:
            print(f"\n{'='*60}")
            print(f"NASA PO.DAAC — {ds.upper()}")
            print(f"  BBOX: {bbox}")
            print(f"  Range: {start} to {end}")
            print(f"{'='*60}")

            granules = self.search(bbox, start, end, ds, max_results)
            if not granules:
                print(f"  No {ds} granules found.")
                continue

            print(f"  Found {len(granules)} granules")
            for i, g in enumerate(granules):
                print(f"\n[{i+1}/{len(granules)}] {g['title']}")
                # Preserve extension from URL — SWOT is .nc, ICESat-2 is .h5
                href = g['href']
                ext = href.rsplit('.', 1)[-1] if '.' in href.rsplit('/', 1)[-1] else 'nc'
                dest = self.output_dir / ds / f"{g['title']}.{ext}"
                if self.dry_run:
                    print(f"  [DRY RUN] Would download: {href}")
                    all_downloaded.append(dest)
                    continue

                dest.parent.mkdir(parents=True, exist_ok=True)
                resp = earthdata_authenticated_get(href, self.session, timeout=600)
                if resp is None or resp.status_code != 200:
                    code = resp.status_code if resp is not None else 'no response'
                    print(f"  ⚠ Download failed: {code}")
                    continue
                with open(dest, 'wb') as f:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        f.write(chunk)
                print(f"  Downloaded: {dest.name} ({dest.stat().st_size / 1e6:.1f} MB)")
                all_downloaded.append(dest)

        print(f"\n  Total downloaded: {len(all_downloaded)} files")
        return all_downloaded


# ── Source 4: USGS Earth Explorer (Landsat 8/9) ──────────────────────────

class USGSDownloader:
    """Download Landsat 8/9 from USGS Earth Explorer via M2M API.

    Uses the new M2M JSON API with application token authentication.
    The legacy username/password login endpoint was deprecated Feb 26, 2025.
    """

    API_URL = 'https://m2m.cr.usgs.gov/api/api/json/stable/'

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'usgs')
        self.api_key = cfg('USGS_API_KEY')  # Application token from ERS profile
        # login-token endpoint requires the ERS account username alongside the token.
        self.username = (
            cfg('USGS_M2M_USERNAME')
            or cfg('USGS_USERNAME')
            or cfg('EARTHDATA_USERNAME')
        )
        self.session_token = None
        self.session = requests.Session()
        self.session.headers.update({
            'User-Agent': 'CESAROPS-WreckHunter2000/1.0',
            'Accept': 'application/json',
            'Content-Type': 'application/json',
        })

    def _login(self) -> Optional[str]:
        """Authenticate with M2M API using application token.

        Returns session token for subsequent API calls.
        The login-token endpoint requires both the ERS username and the 64-bit
        encrypted application token (no ERS password needed).
        """
        if not self.api_key:
            print("  ⚠ USGS_API_KEY (application token) not set in .env")
            return None
        if not self.username:
            print("  ⚠ USGS_M2M_USERNAME (ERS account username) not set in .env")
            return None

        if self.session_token:
            return self.session_token

        url = f'{self.API_URL}login-token'
        payload = {'username': self.username, 'token': self.api_key}
        try:
            resp = self.session.post(url, json=payload, timeout=30)
            resp.raise_for_status()
            result = resp.json()

            # Check for errors
            if result.get('error'):
                print(f"  ⚠ USGS login error: {result['error']}")
                return None
            if result.get('errorCode'):
                print(f"  ⚠ USGS login error: {result.get('errorMessage') or result['errorCode']}")
                return None
            if result.get('success') is False:
                print(f"  ⚠ USGS login failed: {result.get('errorMessage') or 'request failed'}")
                return None

            # M2M login-token returns the API session token directly in `data`
            # (a plain string), unlike some endpoints that nest it under data.token.
            data = result.get('data')
            if isinstance(data, dict):
                self.session_token = data.get('token')
            elif isinstance(data, str):
                self.session_token = data
            else:
                self.session_token = None
            if not self.session_token:
                print("  ⚠ USGS login failed: no token returned")
                return None

            print(f"  [USGS] Authenticated with application token")
            return self.session_token

        except Exception as e:
            print(f"  ⚠ USGS login exception: {e}")
            return None

    def _call(self, endpoint: str, data: dict = None) -> dict:
        """Make authenticated M2M API call."""
        # Ensure we're authenticated first
        if not self._login():
            raise RuntimeError("USGS M2M authentication failed")

        url = f'{self.API_URL}{endpoint}'
        payload = data or {}
        headers = {'X-Auth-Token': self.session_token}
        resp = self.session.post(url, json=payload, headers=headers, timeout=30)
        resp.raise_for_status()
        result = resp.json()

        # Check for errors
        if result.get('error'):
            raise RuntimeError(f"USGS API error: {result['error']}")
        if result.get('errorCode'):
            raise RuntimeError(f"USGS API error: {result.get('errorMessage') or result['errorCode']}")
        if result.get('success') is False:
            raise RuntimeError(f"USGS API error: {result.get('errorMessage') or 'request failed'}")

        return result.get('data', {})

    def search(self, bbox: List[float], start: str, end: str,
               dataset: str = 'landsat_ot_c2_l2',
               max_results: int = 50) -> List[Dict]:
        """Search for Landsat scenes."""
        if not self.api_key:
            print("  ⚠ USGS_API_KEY not set in .env")
            return []

        result = self._call('scene-search', {
            'datasetName': dataset,
            'sceneFilter': {
                'acquisitionFilter': {
                    'start': start,
                    'end': end,
                },
                'spatialFilter': {
                    'filterType': 'mbr',
                    'lowerLeft': {'longitude': bbox[1], 'latitude': bbox[0]},
                    'upperRight': {'longitude': bbox[3], 'latitude': bbox[2]},
                },
            },
            'maxResults': max_results,
        })

        scenes = []
        for scene in result.get('results', []):
            scenes.append({
                'id': scene.get('entityId', ''),
                'display_id': scene.get('displayId', ''),
                'browse_path': scene.get('browse', [{}])[0].get('thumbnailPath', ''),
                'acquisition_date': scene.get('acquisitionDate', ''),
                'cloud_cover': scene.get('cloudCover', 0),
                'metadata': scene.get('metadata', []),
            })
        return scenes

    def get_download_options(self, entity_id: str,
                             dataset: str = 'landsat_ot_c2_l2') -> List[Dict]:
        """Get download options for a scene.

        `_call` already unwraps the top-level `data` field, which for
        download-options is the list of product option dicts.
        """
        result = self._call('download-options', {
            'datasetName': dataset,
            'entityIds': [entity_id],
        })
        if isinstance(result, list):
            return result
        if isinstance(result, dict):
            return result.get('data', [])
        return []

    def request_download(self, entity_id: str, dataset: str = 'landsat_ot_c2_l2',
                         product_id: str = '') -> str:
        """Request a download URL for a scene's Level-2 product bundle.

        Product IDs are not stable across scenes, so when one isn't supplied we
        query download-options and pick the first *available* full Product
        Bundle (falling back to any available product).
        """
        if not product_id:
            product_id = self._pick_product_id(entity_id, dataset)
            if not product_id:
                print(f"  ⚠ No available download product for {entity_id}")
                return ''

        result = self._call('download-request', {
            'downloads': [{
                'entityId': entity_id,
                'datasetName': dataset,
                'productId': product_id,
            }],
        })
        # _call already unwraps `data`; M2M returns preparingDownloads and
        # availableDownloads lists, each item carrying a direct 'url' key.
        if not isinstance(result, dict):
            return ''
        for item in result.get('availableDownloads', []):
            url = item.get('url', '')
            if url:
                return url
        for item in result.get('preparingDownloads', []):
            url = item.get('url', '')
            if url:
                return url
        return ''

    def _pick_product_id(self, entity_id: str,
                         dataset: str = 'landsat_ot_c2_l2') -> str:
        """Select an available product ID, preferring the full Product Bundle."""
        opts = self.get_download_options(entity_id, dataset)
        bundle = ''
        for o in opts:
            if not o.get('available'):
                continue
            name = (o.get('productName') or '').lower()
            if 'bundle' in name:
                return o.get('id', '')
            if not bundle:
                bundle = o.get('id', '')
        return bundle

    def run(self, bbox: List[float], start: str, end: str,
            max_results: int = 20) -> List[Path]:
        """Search and download Landsat scenes."""
        print(f"\n{'='*60}")
        print(f"USGS Earth Explorer — Landsat 8/9")
        print(f"  BBOX: {bbox}")
        print(f"  Range: {start} to {end}")
        print(f"{'='*60}")

        scenes = self.search(bbox, start, end, max_results=max_results)
        if not scenes:
            print("  No scenes found.")
            return []

        print(f"  Found {len(scenes)} scenes")
        downloaded = []
        for i, s in enumerate(scenes):
            print(f"\n[{i+1}/{len(scenes)}] {s['display_id']} (cloud: {s['cloud_cover']}%)")
            if self.dry_run:
                downloaded.append(self.output_dir / f"{s['display_id']}.tar.gz")
                continue

            url = self.request_download(s['id'])
            if url:
                dest = self.output_dir / f"{s['display_id']}.tar.gz"
                dest.parent.mkdir(parents=True, exist_ok=True)
                resp = self.session.get(url, stream=True, timeout=600)
                resp.raise_for_status()
                with open(dest, 'wb') as f:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        f.write(chunk)
                print(f"  Downloaded: {dest.name} ({dest.stat().st_size / 1e6:.1f} MB)")
                downloaded.append(dest)

        print(f"\n  Total downloaded: {len(downloaded)} files")
        return downloaded


# ── Source 5: NASA HLS (Harmonized Landsat Sentinel-2) ────────────────────

class HLSDownloader:
    """Download HLS (Harmonized Landsat Sentinel-2) from NASA LP DAAC.

    Fallback chain:
      1. NASA CMR — HLSS30 / HLSL30 .tif bands (LP DAAC S3 via temp credentials)
      2. AWS Element84 STAC — Sentinel-2 L2A COG (free, no auth)
      3. AWS Element84 STAC — Landsat C2 L2 COG (free, no auth)
    """

    CMR_URL = 'https://cmr.earthdata.nasa.gov/search/granules.json'
    STAC_URL = 'https://earth-search.aws.element84.com/v1/search'
    S3CREDS_URL = 'https://data.lpdaac.earthdatacloud.nasa.gov/s3credentials'
    LP_DAAC_BUCKET = 'lp-prod-protected'
    CMR_STAC_COLLECTIONS = {
        'HLSS30': 'HLSS30_2.0',
        'HLSL30': 'HLSL30_2.0',
    }

    # Angle/QA bands to skip — download only spectral + cloud mask
    _SKIP_BANDS = {'SAA', 'SZA', 'VAA', 'VZA'}
    # Spectral bands to prefer for HLS (Sentinel-2 naming + Landsat overlap)
    _PREFER_BANDS = {'B02', 'B03', 'B04', 'B05', 'B06', 'B07', 'B8A', 'B09', 'B11', 'B12', 'Fmask'}

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'hls')
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})
        self.cmrstac = CMRSTACClient('LPCLOUD')

    def search(self, bbox: List[float], start: str, end: str,
               product: str = 'HLSS30',
               max_results: int = 50) -> List[Dict]:
        """Search HLS granules via CMR-STAC first, then CMR granules API fallback."""
        stac_coll = self.CMR_STAC_COLLECTIONS.get(product)
        if stac_coll:
            features = self.cmrstac.search_items([stac_coll], bbox, start, end, limit=max_results)
            stac_granules: List[Dict] = []
            for feat in features:
                title = feat.get('id', '')
                time_start = (feat.get('properties') or {}).get('datetime', '')
                for band, asset in feat.get('assets', {}).items():
                    if band in self._SKIP_BANDS:
                        continue
                    if self._PREFER_BANDS and band not in self._PREFER_BANDS:
                        continue
                    href = (asset.get('href') or '') if isinstance(asset, dict) else ''
                    if not href.endswith('.tif'):
                        continue
                    stac_granules.append({
                        'id': feat.get('id', ''),
                        'title': title,
                        'band': band,
                        'href': href,
                        'time_start': time_start,
                        'source': 'cmr_stac',
                    })
            if stac_granules:
                return stac_granules

        params = {
            'short_name': product,
            'bounding_box': f'{bbox[1]},{bbox[0]},{bbox[3]},{bbox[2]}',
            'temporal': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'page_size': max_results,
        }
        # CMR search API is public — Bearer token causes 401; use plain requests
        resp = requests.get(self.CMR_URL, params=params,
                            headers={'User-Agent': 'CESAROPS-WreckHunter2000/1.0'},
                            timeout=30)
        if resp.status_code != 200:
            print(f"  ⚠ CMR search failed: {resp.status_code}")
            return []

        granules = []
        for entry in resp.json().get('feed', {}).get('entry', []):
            title = entry.get('title', '')
            time_start = entry.get('time_start', '')
            for link in entry.get('links', []):
                href = link.get('href', '')
                if not href.endswith('.tif'):
                    continue
                # CMR may return relative paths — resolve against LP DAAC CDN base
                if href.startswith('/'):
                    href = 'https://data.lpdaac.earthdatacloud.nasa.gov' + href
                # Extract band name from filename: title.BAND.tif
                fname = href.rsplit('/', 1)[-1]
                band = fname.rsplit('.', 2)[-2] if fname.count('.') >= 2 else ''
                if band in self._SKIP_BANDS:
                    continue
                granules.append({
                    'id': entry.get('id', ''),
                    'title': title,
                    'band': band,
                    'href': href,
                    'time_start': time_start,
                    'source': 'cmr',
                })
        return granules

    def _stac_fallback(self, bbox: List[float], start: str, end: str,
                       max_results: int = 10) -> List[Dict]:
        """AWS Element84 STAC fallback — free, no auth required."""
        granules = []
        # Sentinel-2 L2A COGs
        # Key bands for wreck/HC detection:
        #   blue   = B02 (water-penetrating optical, 458-523 nm)
        #   green  = B03 (Stumpf bathymetry partner)
        #   red    = B04 (surface reference + HC cross-check)
        #   swir16 = B11 (1565 nm, HC/oil absorbs SWIR → dark anomaly)
        # Omitted to save bandwidth: nir/nir08 (surface-only), swir22, scl, qa_pixel
        for coll, bands in [
            ('sentinel-2-l2a', ['blue', 'green', 'red', 'swir16']),
            ('landsat-c2-l2',  ['blue', 'green', 'red', 'swir16']),
        ]:
            try:
                payload = {
                    'collections': [coll],
                    'bbox': [bbox[1], bbox[0], bbox[3], bbox[2]],
                    'datetime': f'{start}T00:00:00Z/{end}T23:59:59Z',
                    'limit': max_results,
                    'query': {'eo:cloud_cover': {'lt': 50}},
                    'sortby': [{'field': 'properties.eo:cloud_cover', 'direction': 'asc'}],
                }
                resp = requests.post(self.STAC_URL, json=payload, timeout=20)
                if resp.status_code != 200:
                    continue
                for feat in resp.json().get('features', []):
                    feat_id = feat['id']
                    assets = feat.get('assets', {})
                    for band_name in bands:
                        asset = assets.get(band_name)
                        if not asset:
                            continue
                        href = asset.get('href', '')
                        # Convert s3:// to HTTPS for direct download
                        if href.startswith('s3://usgs-landsat/'):
                            href = href.replace('s3://usgs-landsat/', 'https://usgs-landsat.s3.us-west-2.amazonaws.com/', 1)
                        elif href.startswith('s3://sentinel-cogs/'):
                            href = href.replace('s3://sentinel-cogs/', 'https://sentinel-cogs.s3.us-west-2.amazonaws.com/', 1)
                        granules.append({
                            'id': feat_id,
                            'title': feat_id,
                            'band': band_name,
                            'href': href,
                            'time_start': feat['properties'].get('datetime', ''),
                            'source': coll,
                        })
            except Exception as e:
                print(f"  ⚠ STAC fallback ({coll}): {e}")
        return granules

    def _get_s3_client(self):
        """Get a boto3 S3 client using temporary LP DAAC credentials."""
        try:
            import boto3
        except ImportError:
            return None
        try:
            resp = self._earthdata_get(self.S3CREDS_URL, timeout=20)
            if resp is None or resp.status_code != 200:
                status = getattr(resp, 'status_code', 'no-response')
                print(f"  ⚠ S3 credentials failed: {status}")
                return None
            creds = resp.json()
            return boto3.client(
                's3',
                region_name='us-west-2',
                aws_access_key_id=creds['accessKeyId'],
                aws_secret_access_key=creds['secretAccessKey'],
                aws_session_token=creds['sessionToken'],
            )
        except Exception as e:
            print(f"  ⚠ S3 client error: {e}")
            return None

    def _earthdata_get(self, url: str, timeout: int = 600):
        """GET with Earthdata auth.

        Uses earthaccess authenticated session (URS cookie dance) when available.
        Falls back to manual redirect following with Basic auth for the URS
        OAuth challenge and auth-free requests to S3 presigned URLs.
        """
        # ── Lazy-init earthaccess session ──────────────────────────────────
        if not hasattr(self, '_ea_session'):
            self._ea_session = None
            eu = cfg('EARTHDATA_USERNAME', '')
            ep = cfg('EARTHDATA_PASSWORD', '')
            if eu and ep:
                try:
                    import earthaccess as _ea
                    _ea.login(strategy='environment')
                    # get_requests_https_session returns a properly-cookied Session
                    if hasattr(_ea, 'get_requests_https_session'):
                        self._ea_session = _ea.get_requests_https_session()
                    else:
                        auth = _ea.login(strategy='environment')
                        if hasattr(auth, 'get_session'):
                            self._ea_session = auth.get_session()
                    if self._ea_session is not None:
                        print('  [auth] earthaccess HTTPS session ready')
                except Exception as _ea_err:
                    print(f'  [auth] earthaccess init failed: {_ea_err}')

        # ── Use earthaccess session ─────────────────────────────────────────
        if self._ea_session is not None:
            try:
                resp = self._ea_session.get(url, stream=True, timeout=timeout)
                return resp
            except Exception as _se:
                print(f'  ⚠ earthaccess get error: {_se}')
                # fall through to manual

        # ── Manual redirect following ───────────────────────────────────────
        eu = cfg('EARTHDATA_USERNAME', '')
        ep = cfg('EARTHDATA_PASSWORD', '')
        current_url = url
        for _ in range(12):
            if 's3.amazonaws.com' in current_url or 'X-Amz-Signature' in current_url:
                # S3 presigned URL — MUST NOT send auth headers (breaks sig)
                resp = requests.get(current_url, stream=True, timeout=timeout,
                                    allow_redirects=True)
                return resp
            if 'urs.earthdata.nasa.gov' in current_url:
                if eu and ep:
                    resp = requests.get(current_url, auth=(eu, ep), timeout=timeout,
                                        allow_redirects=False)
                else:
                    resp = self.session.get(current_url, allow_redirects=False,
                                            timeout=timeout)
            else:
                resp = self.session.get(current_url, allow_redirects=False,
                                        timeout=timeout)
            if resp.status_code in (301, 302, 303, 307, 308):
                loc = resp.headers.get('Location', '')
                if not loc:
                    return None
                current_url = urljoin(current_url, loc)
                continue
            return resp
        return None

    def run(self, bbox: List[float], start: str, end: str,
            max_results: int = 20, products: List[str] = None) -> List[Path]:
        """Search and download HLS band GeoTIFFs with AWS STAC fallback."""
        if products is None:
            products = ['HLSS30', 'HLSL30']

        print(f"\n{'='*60}")
        print(f"NASA HLS — Harmonized Landsat Sentinel-2")
        print(f"  BBOX: {bbox}  |  {start} → {end}")
        print(f"{'='*60}")

        granules: List[Dict] = []
        for prod in products:
            print(f"  Searching CMR for {prod}...")
            hits = self.search(bbox, start, end, product=prod, max_results=max_results)
            print(f"  CMR {prod}: {len(hits)} band files found")
            granules.extend(hits)

        if not granules:
            print("  ⚠ No HLS granules via CMR — trying AWS STAC fallback (Sentinel-2/Landsat COG)...")
            granules = self._stac_fallback(bbox, start, end, max_results=max_results // 2)
            if granules:
                print(f"  AWS STAC: {len(granules)} band files found")
            else:
                print("  No data found from any source.")
                return []

        downloaded = []
        for i, g in enumerate(granules):
            band = g.get('band', 'band')
            fname = f"{g['title']}.{band}.tif"
            dest = self.output_dir / fname
            src_tag = g.get('source', 'cmr')

            if self.dry_run:
                print(f"  [DRY RUN] [{src_tag}] {fname}")
                downloaded.append(dest)
                continue
            if dest.exists() and dest.stat().st_size > 0:
                print(f"  [skip] {dest.name}")
                downloaded.append(dest)
                continue

            dest.parent.mkdir(parents=True, exist_ok=True)
            url = g['href']

            # Extract S3 bucket/key from LP DAAC paths for direct S3 download
            s3_key = None
            if url.startswith('https://data.lpdaac.earthdatacloud.nasa.gov/lp-prod-protected/'):
                s3_key = url.replace(
                    'https://data.lpdaac.earthdatacloud.nasa.gov/lp-prod-protected/', '', 1)
            elif url.startswith('/lp-prod-protected/'):
                s3_key = url[len('/lp-prod-protected/'):]
            elif url.startswith('s3://lp-prod-protected/'):
                s3_key = url[len('s3://lp-prod-protected/'):]

            if s3_key:
                # Use temporary S3 credentials obtained via Bearer JWT token
                # (_s3_tried == True means we already tried and it failed)
                if not hasattr(self, '_s3_tried'):
                    self._s3_tried = False
                    self._s3_client = None
                if not self._s3_tried:
                    self._s3_client = self._get_s3_client()
                    self._s3_tried = True
                if self._s3_client is not None:
                    try:
                        self._s3_client.download_file(
                            self.LP_DAAC_BUCKET, s3_key, str(dest))
                        sz = dest.stat().st_size
                        print(f"  [{i+1}/{len(granules)}] {dest.name} ({sz/1e6:.1f} MB) [s3]")
                        downloaded.append(dest)
                        continue
                    except Exception as _s3e:
                        print(f"  ⚠ S3 download fail for {band}: {_s3e}")
                        # Fall through to HTTP
                else:
                    print(f"  ⚠ No S3 client, trying HTTP fallback")

            # HTTP fallback: convert s3:// → HTTPS for public buckets
            if url.startswith('s3://lp-prod-protected/'):
                url = url.replace('s3://lp-prod-protected/',
                                  'https://data.lpdaac.earthdatacloud.nasa.gov/lp-prod-protected/', 1)
            elif url.startswith('s3://sentinel-cogs/'):
                url = url.replace('s3://sentinel-cogs/',
                                  'https://sentinel-cogs.s3.us-west-2.amazonaws.com/', 1)
            elif url.startswith('/lp-prod-protected/'):
                url = 'https://data.lpdaac.earthdatacloud.nasa.gov' + url

            # For public S3 (sentinel-cogs), download directly
            if 'sentinel-cogs.s3' in url:
                resp = requests.get(url, stream=True, timeout=600)
            else:
                resp = self._earthdata_get(url)
            if resp is None or resp.status_code != 200:
                code = resp.status_code if resp else 'ERR'
                print(f"  ⚠ [{url[:80]}]: HTTP {code}")
                continue
            with open(dest, 'wb') as f:
                for chunk in resp.iter_content(chunk_size=1 << 20):
                    f.write(chunk)
            print(f"  [{i+1}/{len(granules)}] {dest.name} ({dest.stat().st_size/1e6:.1f} MB) [{src_tag}]")
            downloaded.append(dest)

        print(f"\n  Total downloaded: {len(downloaded)} files")
        return downloaded


# (SentinelHubDownloader removed — requires paid commercial subscription, replaced by AWSSentinel2Downloader/FEDEODownloader)

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'sentinel_hub')
        self.client_id = cfg('SENTINEL_HUB_CLIENT_ID')
        self.client_secret = cfg('SENTINEL_HUB_CLIENT_SECRET')
        self.session = requests.Session()
        self._token = None
        self._token_expiry = 0

    def _get_token(self) -> Optional[str]:
        if not self.client_id or not self.client_secret:
            print("  ⚠ SENTINEL_HUB_CLIENT_ID/SECRET not set in .env")
            return None
        if self._token and time.time() < self._token_expiry:
            return self._token

        resp = requests.post('https://services.sentinel-hub.com/oauth/token', data={
            'grant_type': 'client_credentials',
            'client_id': self.client_id,
            'client_secret': self.client_secret,
        })
        if resp.status_code != 200:
            print(f"  ⚠ Sentinel Hub auth failed: {resp.status_code}")
            return None

        data = resp.json()
        self._token = data['access_token']
        self._token_expiry = time.time() + data.get('expires_in', 3600) - 60
        self.session.headers.update({'Authorization': f'Bearer {self._token}'})
        return self._token

    def download_image(self, bbox: List[float], start: str, end: str,
                       bands: str = 'B04,B08', resolution: float = 10.0,
                       maxcc: float = 0.2, img_format: str = 'image/tiff') -> Optional[Path]:
        """Request processed image from Sentinel Hub."""
        if not self._get_token():
            return None

        evalscript = f"""
        //VERSION=3
        function setup() {{
            return {{
                input: ["B04", "B08", "dataMask"],
                output: {{ bands: 4, sampleType: SampleType.FLOAT32 }}
            }};
        }}
        function evaluatePixel(samples) {{
            let s = samples[0];
            return [s.B04, s.B08, s.B04 / s.B08, s.dataMask];
        }}
        """

        payload = {
            'input': {
                'bounds': {
                    'bbox': [bbox[1], bbox[0], bbox[3], bbox[2]],
                    'properties': {'crs': 'http://www.opengis.net/def/crs/EPSG/0/4326'},
                },
                'data': [{
                    'type': 'sentinel-2-l2a',
                    'dataFilter': {
                        'timeRange': {'from': f'{start}T00:00:00Z', 'to': f'{end}T23:59:59Z'},
                        'maxCloudCoverage': maxcc,
                    },
                }],
            },
            'output': {
                'width': int((bbox[3] - bbox[1]) * 111320 / resolution),
                'height': int((bbox[2] - bbox[0]) * 111320 / resolution),
                'responses': [{'identifier': 'default', 'format': {'type': img_format}}],
            },
            'evalscript': evalscript,
        }

        if self.dry_run:
            dest = self.output_dir / f"sh_{start}_{end}.tif"
            print(f"  [DRY RUN] Would request {payload['output']['width']}x{payload['output']['height']} image")
            return dest

        resp = self.session.post(self.PROCESS_URL, json=payload, timeout=300)
        if resp.status_code != 200:
            print(f"  ⚠ Process failed: {resp.status_code} {resp.text[:200]}")
            return None

        dest = self.output_dir / f"sh_{start}_{end}.tif"
        dest.parent.mkdir(parents=True, exist_ok=True)

# ── Source 7: NOAA CoastWatch ERDDAP (Great Lakes SST, free, no auth) ────────

class NOAACoastwatchDownloader:
    """Download Great Lakes Surface Environmental Analysis (GLSEA) SST from NOAA CoastWatch.

    Free — no authentication. ~1.8 km daily thermal composites of all five Great Lakes.
    AVHRR-based, GOES-supplemented. Best proxy for thermal anomaly before/after Landsat passes.

    Dataset IDs:
      GLSEA_GCS   — Daily SST composite, full Great Lakes (through ~Jan 2024)
      GLSEA2_GCS  — 7-day max cloud-free composite

    ERDDAP variable: sst  (sea_water_temperature, degrees_C)
    Coverage: 1995-01-01 → 2024-01-01 (AVHRR archive)
    """

    ERDDAP_BASE = 'https://apps.glerl.noaa.gov/erddap/griddap'
    DATASET = 'GLSEA_GCS'
    # Dataset ends ~2024-01-01; cap requests to avoid 404
    MAX_DATE = '2024-01-01'

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'noaa_coastwatch')
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    def search(self, bbox: List[float], start: str, end: str) -> List[Dict]:
        """Return a list of date strings that should have SST coverage."""
        from datetime import date as _date
        d0 = datetime.strptime(start, '%Y-%m-%d').date()
        d1 = datetime.strptime(min(end, self.MAX_DATE), '%Y-%m-%d').date()
        if d0 > d1:
            print(f"  ⚠ GLSEA dataset ends {self.MAX_DATE} — no coverage for {start}→{end}")
            return []
        dates = []
        d = d0
        while d <= d1:
            dates.append(d.strftime('%Y-%m-%d'))
            d += timedelta(days=7)  # Weekly samples to avoid huge volumes
        return [{'date': s} for s in dates]

    def download_sst(self, date_str: str, bbox: List[float]) -> Optional[Path]:
        """Download a single day's SST netCDF for the given bbox."""
        lat_min, lon_min, lat_max, lon_max = bbox[0], bbox[1], bbox[2], bbox[3]
        # ERDDAP griddap URL — variable is 'sst', not 'surface_temp'
        url = (
            f"{self.ERDDAP_BASE}/{self.DATASET}.nc"
            f"?sst"
            f"[({date_str}T12:00:00Z):1:({date_str}T12:00:00Z)]"
            f"[({lat_min}):1:({lat_max})]"
            f"[({lon_min}):1:({lon_max})]"
        )
        dest = self.output_dir / f"glsea_sst_{date_str}.nc"
        if self.dry_run:
            print(f"  [DRY RUN] Would download: {dest.name}")
            return dest
        if dest.exists() and dest.stat().st_size > 0:
            print(f"  [skip] {dest.name}")
            return dest

        dest.parent.mkdir(parents=True, exist_ok=True)
        resp = self.session.get(url, timeout=120)
        if resp.status_code != 200:
            print(f"  ⚠ GLSEA SST {date_str}: HTTP {resp.status_code}")
            return None
        with open(dest, 'wb') as f:
            f.write(resp.content)
        print(f"  Downloaded: {dest.name} ({dest.stat().st_size / 1e6:.2f} MB)")
        return dest

    def run(self, bbox: List[float], start: str, end: str, **_) -> List[Path]:
        print(f"\n{'='*60}")
        print(f"NOAA CoastWatch ERDDAP — Great Lakes SST (GLSEA, free)")
        print(f"  BBOX: {bbox}  |  {start} → {end}")
        print(f"{'='*60}")
        dates = self.search(bbox, start, end)
        print(f"  {len(dates)} sample dates")
        downloaded = []
        for entry in dates:
            result = self.download_sst(entry['date'], bbox)
            if result:
                downloaded.append(result)
        print(f"\n  Total: {len(downloaded)} SST files")
        return downloaded


# ── Source 8: MODIS Daily Thermal (MOD11A1 / MYD11A1) via NASA CMR ───────────

class MODISDownloader:
    """Download MODIS/VIIRS land surface temperature (LST) from NASA LP DAAC.

        Products:
            MOD11A1   — Terra daily 1-km LST (Day/Night) — Earthdata auth
            MYD11A1   — Aqua daily 1-km LST (Day/Night)  — Earthdata auth
            VNP21A1D  — VIIRS daily 1-km LST (day)       — Earthdata auth
            VNP21A1N  — VIIRS daily 1-km LST (night)     — Earthdata auth

    LST_Day_1km and LST_Night_1km bands cover Great Lakes thermocline
    and shallow nearshore thermal anomalies at 1 km resolution.
    """

    CMR_URL = 'https://cmr.earthdata.nasa.gov/search/granules.json'

    COLLECTIONS = {
        'mod11a1': {'short_name': 'MOD11A1', 'version': '061', 'stac_id': 'MOD11A1_061'},
        'myd11a1': {'short_name': 'MYD11A1', 'version': '061', 'stac_id': 'MYD11A1_061'},
        'vnp21a1d': {'short_name': 'VNP21A1D', 'version': '002', 'stac_id': 'VNP21A1D_002'},
        'vnp21a1n': {'short_name': 'VNP21A1N', 'version': '002', 'stac_id': 'VNP21A1N_002'},
        'viirs_lst': {'short_name': 'VNP21A1D', 'version': '002', 'stac_id': 'VNP21A1D_002'},  # legacy alias
    }

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'modis')
        self.session = earthdata_session()
        self.cmrstac = CMRSTACClient('LPCLOUD')

    def search(self, bbox: List[float], start: str, end: str,
               product: str = 'mod11a1', max_results: int = 30) -> List[Dict]:
        coll = self.COLLECTIONS.get(product.lower())
        if not coll:
            print(f"  Unknown MODIS product: {product}")
            return []

        # Prefer CMR-STAC provider search for collection-scoped item discovery.
        stac_id = coll.get('stac_id')
        if stac_id:
            features = self.cmrstac.search_items([stac_id], bbox, start, end, limit=max_results)
            stac_granules: List[Dict] = []
            for feat in features:
                title = feat.get('id', '')
                time_start = (feat.get('properties') or {}).get('datetime', '')
                hrefs = self.cmrstac.pick_asset_hrefs(feat.get('assets', {}), exts=('.hdf', '.h5', '.nc'))
                for href in hrefs:
                    stac_granules.append({
                        'title': title,
                        'href': href,
                        'time_start': time_start,
                        'product': product,
                    })
            if stac_granules:
                return stac_granules

        params = {
            'short_name': coll['short_name'],
            'version': coll['version'],
            'bounding_box': f'{bbox[1]},{bbox[0]},{bbox[3]},{bbox[2]}',
            'temporal': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'page_size': max_results,
        }
        resp = self.session.get(self.CMR_URL, params=params)
        if resp.status_code != 200:
            print(f"  ⚠ CMR search failed: {resp.status_code}")
            return []
        granules = []
        for entry in resp.json().get('feed', {}).get('entry', []):
            for link in entry.get('links', []):
                href = link.get('href', '')
                if href.endswith('.hdf') or href.endswith('.nc') or href.endswith('.h5'):
                    granules.append({
                        'title': entry.get('title', ''),
                        'href': href,
                        'time_start': entry.get('time_start', ''),
                        'product': product,
                    })
                    break
        return granules

    def run(self, bbox: List[float], start: str, end: str,
            products: List[str] = None, max_results: int = 20) -> List[Path]:
        if products is None:
            products = ['mod11a1', 'myd11a1']
        all_downloaded = []
        for prod in products:
            print(f"\n{'='*60}")
            print(f"LST Thermal — {prod.upper()} (1km daily)")
            print(f"  BBOX: {bbox}  |  {start} → {end}")
            print(f"{'='*60}")
            granules = self.search(bbox, start, end, prod, max_results)
            if not granules:
                print(f"  No granules found (check EARTHDATA_TOKEN in .env)")
                continue
            print(f"  Found {len(granules)} granules")
            for i, g in enumerate(granules):
                suffix = g['href'].rsplit('.', 1)[-1]
                dest = self.output_dir / prod / f"{g['title']}.{suffix}"
                if self.dry_run:
                    print(f"  [DRY RUN] {g['title']}")
                    all_downloaded.append(dest)
                    continue
                if dest.exists() and dest.stat().st_size > 0:
                    print(f"  [skip] {dest.name}")
                    all_downloaded.append(dest)
                    continue
                dest.parent.mkdir(parents=True, exist_ok=True)
                resp = self.session.get(g['href'], stream=True, timeout=600)
                if resp.status_code != 200:
                    print(f"  ⚠ {resp.status_code}")
                    continue
                with open(dest, 'wb') as f:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        f.write(chunk)
                print(f"  [{i+1}/{len(granules)}] {dest.name} ({dest.stat().st_size/1e6:.1f} MB)")
                all_downloaded.append(dest)
        print(f"\n  Total: {len(all_downloaded)} LST files")
        return all_downloaded


# ── Source 9: ICESat-2 ATL03 (photon-level water, Earthdata) ─────────────────

class ICESat2Downloader:
    """Download ICESat-2 ATL03 (geolocated photons) for water-column penetration.

    ATL03 provides individual photon returns — useful for shallow-water bathymetry
    in the Great Lakes where water clarity allows ~15-25m penetration.
    ATL13 (inland water surface height) is already in PODAACDownloader.

    Uses existing EARTHDATA_TOKEN.
    """

    CMR_URL = 'https://cmr.earthdata.nasa.gov/search/granules.json'

    COLLECTIONS = {
        'atl03': {'short_name': 'ATL03'},  # Geolocated photons
        'atl08': {'short_name': 'ATL08'},  # Land/canopy height
        'atl13': {'short_name': 'ATL13'},  # Inland water height
    }

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'icesat2')
        self.session = earthdata_session()

    def search(self, bbox: List[float], start: str, end: str,
               product: str = 'atl13', max_results: int = 20) -> List[Dict]:
        coll = self.COLLECTIONS.get(product.lower())
        if not coll:
            print(f"  Unknown ICESat-2 product: {product}")
            return []
        params = {
            'short_name': coll['short_name'],
            'bounding_box': f'{bbox[1]},{bbox[0]},{bbox[3]},{bbox[2]}',
            'temporal': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'page_size': max_results,
        }
        if coll.get('version'):
            params['version'] = coll['version']
        resp = self.session.get(self.CMR_URL, params=params)
        if resp.status_code != 200:
            print(f"  ⚠ CMR search failed: {resp.status_code}")
            return []
        granules = []
        for entry in resp.json().get('feed', {}).get('entry', []):
            for link in entry.get('links', []):
                href = link.get('href', '')
                if href.endswith('.h5') or href.endswith('.nc'):
                    granules.append({
                        'title': entry.get('title', ''),
                        'href': href,
                        'time_start': entry.get('time_start', ''),
                        'product': product,
                    })
                    break
        return granules

    def run(self, bbox: List[float], start: str, end: str,
            products: List[str] = None, max_results: int = 20) -> List[Path]:
        if products is None:
            products = ['atl13', 'atl03']
        all_downloaded = []
        for prod in products:
            print(f"\n{'='*60}")
            print(f"ICESat-2 — {prod.upper()}")
            print(f"  BBOX: {bbox}  |  {start} → {end}")
            print(f"{'='*60}")
            granules = self.search(bbox, start, end, prod, max_results)
            if not granules:
                print(f"  No granules (check EARTHDATA_TOKEN in .env)")
                continue
            print(f"  Found {len(granules)} granules")
            for i, g in enumerate(granules):
                dest = self.output_dir / prod / f"{g['title']}.h5"
                if self.dry_run:
                    print(f"  [DRY RUN] {g['title']}")
                    all_downloaded.append(dest)
                    continue
                if dest.exists() and dest.stat().st_size > 0:
                    print(f"  [skip] {dest.name}")
                    all_downloaded.append(dest)
                    continue
                dest.parent.mkdir(parents=True, exist_ok=True)
                resp = self.session.get(g['href'], stream=True, timeout=600)
                if resp.status_code != 200:
                    print(f"  ⚠ {resp.status_code}")
                    continue
                with open(dest, 'wb') as f:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        f.write(chunk)
                print(f"  [{i+1}/{len(granules)}] {dest.name} ({dest.stat().st_size/1e6:.1f} MB)")
                all_downloaded.append(dest)
        print(f"\n  Total: {len(all_downloaded)} ICESat-2 files")
        return all_downloaded


# ── Source 10: USGS 3DEP LiDAR via The National Map (free, no auth) ──────────

class USGSLiDARDownloader:
    """Download LiDAR point clouds and DEMs from USGS 3DEP via the TNM API.

    Free — no authentication required.
    Covers all Great Lakes shorelines with 1m resolution LiDAR returns.

    Products available:
      - Lidar Point Cloud (LPC) — raw .laz point clouds
      - Digital Elevation Model (DEM 1m) — processed bare-earth and first-return
    """

    TNM_URL = 'https://tnmapi.cr.usgs.gov/api/products'

    # Great Lakes shoreline focus (slightly inland + nearshore)
    DATASETS = {
        'lpc': 'Lidar Point Cloud (LPC)',
        'dem_1m': 'Digital Elevation Model (DEM) 1 meter',
        'dem_3dep': '1/3 Arc Second DEM (10m)',
    }

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'lidar')
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    def search(self, bbox: List[float], dataset: str = 'lpc',
               max_results: int = 20) -> List[Dict]:
        """Search TNM for LiDAR products in bbox."""
        params = {
            'datasets': self.DATASETS.get(dataset, self.DATASETS['lpc']),
            'bbox': f'{bbox[1]},{bbox[0]},{bbox[3]},{bbox[2]}',
            'max': max_results,
            'offset': 0,
            'outputFormat': 'JSON',
        }
        resp = self.session.get(self.TNM_URL, params=params, timeout=60)
        if resp.status_code != 200:
            print(f"  ⚠ TNM search failed: {resp.status_code}")
            return []
        data = resp.json()
        items = []
        for item in data.get('items', []):
            dl_url = item.get('downloadURL', '')
            if not dl_url:
                continue
            items.append({
                'title': item.get('title', ''),
                'href': dl_url,
                'size': item.get('sizeInBytes', 0),
                'pub_date': item.get('publicationDate', ''),
                'dataset': dataset,
            })
        return items

    def run(self, bbox: List[float], start: str = '', end: str = '',
            datasets: List[str] = None, max_results: int = 10, **_) -> List[Path]:
        if datasets is None:
            datasets = ['lpc']
        all_downloaded = []
        for ds in datasets:
            print(f"\n{'='*60}")
            print(f"USGS 3DEP LiDAR — {self.DATASETS.get(ds, ds)} (free)")
            print(f"  BBOX: {bbox}")
            print(f"{'='*60}")
            items = self.search(bbox, ds, max_results)
            if not items:
                print(f"  No LiDAR tiles found for this area")
                continue
            print(f"  Found {len(items)} tiles")
            for i, item in enumerate(items):
                fname = item['href'].rsplit('/', 1)[-1] or f"{ds}_{i}.laz"
                dest = self.output_dir / ds / fname
                if self.dry_run:
                    size_mb = item['size'] / 1e6 if item['size'] else 0
                    print(f"  [DRY RUN] {fname} ({size_mb:.0f} MB)")
                    all_downloaded.append(dest)
                    continue
                if dest.exists() and dest.stat().st_size > 0:
                    print(f"  [skip] {dest.name}")
                    all_downloaded.append(dest)
                    continue
                dest.parent.mkdir(parents=True, exist_ok=True)
                resp = self.session.get(item['href'], stream=True, timeout=3600)
                if resp.status_code != 200:
                    print(f"  ⚠ {resp.status_code}")
                    continue
                with open(dest, 'wb') as f:
                    for chunk in resp.iter_content(chunk_size=1 << 20):
                        f.write(chunk)
                print(f"  [{i+1}/{len(items)}] {dest.name} ({dest.stat().st_size/1e6:.1f} MB)")
                all_downloaded.append(dest)
        print(f"\n  Total: {len(all_downloaded)} LiDAR files")
        return all_downloaded


# ── Source 11: AWS Element84 STAC — Sentinel-2 L2A COG (free, no auth) ───────

class AWSSentinel2Downloader:
    """Download Sentinel-2 L2A GeoTIFFs from AWS Open Data via Element84 STAC.

    Free — no authentication. Cloud Optimized GeoTIFFs (COG) from the
    sentinel-cogs S3 bucket (us-west-2, public). 10m–60m resolution.

    Uses Element84 Earth Search STAC at https://earth-search.aws.element84.com/v1
    Same data as Copernicus Open Access Hub but zero-auth.

    Example bands: blue, green, red, nir, nir08, swir16, swir22, scl (cloud)
    """

    STAC_URL = 'https://earth-search.aws.element84.com/v1/search'
    COLLECTION = 'sentinel-2-l2a'
    DEFAULT_BANDS = ['blue', 'green', 'red', 'nir', 'nir08', 'swir16', 'swir22', 'scl']

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None,
                 bands: List[str] = None, max_cloud: float = 30.0):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'sentinel2_aws')
        self.bands = bands or self.DEFAULT_BANDS
        self.max_cloud = max_cloud
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    def search(self, bbox: List[float], start: str, end: str,
               max_results: int = 20) -> List[Dict]:
        """Search Element84 STAC for Sentinel-2 L2A items."""
        payload = {
            'collections': [self.COLLECTION],
            'bbox': [bbox[1], bbox[0], bbox[3], bbox[2]],
            'datetime': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'limit': max_results,
            'query': {'eo:cloud_cover': {'lt': self.max_cloud}},
            'sortby': [{'field': 'properties.eo:cloud_cover', 'direction': 'asc'}],
        }
        resp = self.session.post(self.STAC_URL, json=payload, timeout=30)
        if resp.status_code != 200:
            print(f"  ⚠ Element84 STAC search failed: {resp.status_code}")
            return []

        items = []
        for feat in resp.json().get('features', []):
            feat_id = feat['id']
            cloud = feat['properties'].get('eo:cloud_cover', -1)
            assets = feat.get('assets', {})
            item_bands = []
            for band_name in self.bands:
                asset = assets.get(band_name)
                if not asset:
                    continue
                href = asset.get('href', '')
                if href.startswith('s3://sentinel-cogs/'):
                    href = href.replace('s3://sentinel-cogs/',
                                        'https://sentinel-cogs.s3.us-west-2.amazonaws.com/', 1)
                item_bands.append({
                    'id': feat_id,
                    'title': feat_id,
                    'band': band_name,
                    'href': href,
                    'cloud_cover': cloud,
                    'time_start': feat['properties'].get('datetime', ''),
                })
            items.extend(item_bands)
        return items

    def run(self, bbox: List[float], start: str, end: str,
            max_results: int = 10, **_) -> List[Path]:
        print(f"\n{'='*60}")
        print(f"AWS Sentinel-2 L2A COG — Element84 STAC (free, no auth)")
        print(f"  BBOX: {bbox}  |  {start} → {end}")
        print(f"  Bands: {self.bands}")
        print(f"{'='*60}")

        band_files = self.search(bbox, start, end, max_results=max_results)
        if not band_files:
            print("  No Sentinel-2 scenes found.")
            return []

        unique_scenes = len({b['id'] for b in band_files})
        print(f"  Found {unique_scenes} scenes × {len(self.bands)} bands = {len(band_files)} files")

        downloaded = []
        for bf in band_files:
            fname = f"{bf['title']}.{bf['band']}.tif"
            dest = self.output_dir / fname
            if self.dry_run:
                print(f"  [DRY RUN] {fname}  (cloud {bf['cloud_cover']:.1f}%)")
                downloaded.append(dest)
                continue
            if dest.exists() and dest.stat().st_size > 0:
                print(f"  [skip] {dest.name}")
                downloaded.append(dest)
                continue
            dest.parent.mkdir(parents=True, exist_ok=True)
            resp = self.session.get(bf['href'], stream=True, timeout=600)
            if resp.status_code != 200:
                print(f"  ⚠ {bf['band']} HTTP {resp.status_code}")
                continue
            with open(dest, 'wb') as f:
                for chunk in resp.iter_content(chunk_size=1 << 20):
                    f.write(chunk)
            print(f"  {dest.name} ({dest.stat().st_size/1e6:.1f} MB, cloud {bf['cloud_cover']:.1f}%)")
            downloaded.append(dest)

        print(f"\n  Total: {len(downloaded)} Sentinel-2 COG files")
        return downloaded


# ── Source 12: AWS Element84 STAC — Landsat C2 L2 COG (free, no auth) ────────

class AWSLandsatDownloader:
    """Download Landsat Collection 2 Level-2 GeoTIFFs from USGS on AWS Open Data.

    Free — no authentication required. Cloud Optimized GeoTIFFs (COG) from
    usgs-landsat S3 bucket (us-west-2, public). 30m SR + 100m thermal.

    Uses Element84 Earth Search STAC at https://earth-search.aws.element84.com/v1
    Covers Landsat 4–9 C2 L2A SR+ST.

    Bands: coastal, blue, green, red, nir08, swir16, swir22, st_b10 (thermal),
           qa_pixel (cloud/shadow mask)
    """

    STAC_URL = 'https://earth-search.aws.element84.com/v1/search'
    COLLECTION = 'landsat-c2-l2'
    DEFAULT_BANDS = ['blue', 'green', 'red', 'nir08', 'swir16', 'swir22', 'st_b10', 'qa_pixel']

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None,
                 bands: List[str] = None, max_cloud: float = 30.0):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'landsat_aws')
        self.bands = bands or self.DEFAULT_BANDS
        self.max_cloud = max_cloud
        self.session = requests.Session()
        self.session.headers.update({'User-Agent': 'CESAROPS-WreckHunter2000/1.0'})

    def search(self, bbox: List[float], start: str, end: str,
               max_results: int = 10) -> List[Dict]:
        """Search Element84 STAC for Landsat C2 L2 items."""
        payload = {
            'collections': [self.COLLECTION],
            'bbox': [bbox[1], bbox[0], bbox[3], bbox[2]],
            'datetime': f'{start}T00:00:00Z/{end}T23:59:59Z',
            'limit': max_results,
            'query': {'eo:cloud_cover': {'lt': self.max_cloud}},
            'sortby': [{'field': 'properties.eo:cloud_cover', 'direction': 'asc'}],
        }
        resp = self.session.post(self.STAC_URL, json=payload, timeout=30)
        if resp.status_code != 200:
            print(f"  ⚠ Element84 STAC search failed: {resp.status_code}")
            return []

        items = []
        for feat in resp.json().get('features', []):
            feat_id = feat['id']
            cloud = feat['properties'].get('eo:cloud_cover', -1)
            assets = feat.get('assets', {})
            for band_name in self.bands:
                asset = assets.get(band_name)
                if not asset:
                    continue
                href = asset.get('href', '')
                if href.startswith('s3://usgs-landsat/'):
                    href = href.replace('s3://usgs-landsat/',
                                        'https://usgs-landsat.s3.us-west-2.amazonaws.com/', 1)
                items.append({
                    'id': feat_id,
                    'title': feat_id,
                    'band': band_name,
                    'href': href,
                    'cloud_cover': cloud,
                    'time_start': feat['properties'].get('datetime', ''),
                })
        return items

    def run(self, bbox: List[float], start: str, end: str,
            max_results: int = 10, **_) -> List[Path]:
        print(f"\n{'='*60}")
        print(f"AWS Landsat C2 L2 COG — Element84 STAC (free, no auth)")
        print(f"  BBOX: {bbox}  |  {start} → {end}")
        print(f"  Bands: {self.bands}")
        print(f"{'='*60}")

        band_files = self.search(bbox, start, end, max_results=max_results)
        if not band_files:
            print("  No Landsat scenes found.")
            return []

        unique_scenes = len({b['id'] for b in band_files})
        print(f"  Found {unique_scenes} scenes × bands = {len(band_files)} files")

        downloaded = []
        for bf in band_files:
            fname = f"{bf['title']}.{bf['band']}.tif"
            dest = self.output_dir / fname
            if self.dry_run:
                print(f"  [DRY RUN] {fname}  (cloud {bf['cloud_cover']:.1f}%)")
                downloaded.append(dest)
                continue
            if dest.exists() and dest.stat().st_size > 0:
                print(f"  [skip] {dest.name}")
                downloaded.append(dest)
                continue
            dest.parent.mkdir(parents=True, exist_ok=True)
            resp = self.session.get(bf['href'], stream=True, timeout=600)
            if resp.status_code != 200:
                print(f"  ⚠ {bf['band']} HTTP {resp.status_code}")
                continue
            with open(dest, 'wb') as f:
                for chunk in resp.iter_content(chunk_size=1 << 20):
                    f.write(chunk)
            print(f"  {dest.name} ({dest.stat().st_size/1e6:.1f} MB, cloud {bf['cloud_cover']:.1f}%)")
            downloaded.append(dest)

        print(f"\n  Total: {len(downloaded)} Landsat COG files")
        return downloaded


class FEDEODownloader:
    """Download satellite data via the FEDEO (fedeo.ceos.org) OGC API catalog.

    FEDEO aggregates 3000+ EO collections from CNES PEPS, Copernicus Marine,
    ESA, EUMETSAT, JAXA, and ROSCOSMOS.  Of those, 8 collections are directly
    searchable *and* serve public enclosure download links:

      EOP:SENTINEL-HUB:Sentinel2  – S-2 L1C tiles → public sentinel-s2-l1c S3
      EOP:SENTINEL-HUB:Landsat8   – L8 L1TP scenes → public landsat-pds S3
      EOP:CNES:TAKE5:SPOT4/SPOT5  – SPOT archive (CNES account required)
      EOP:CNES:PEPS:S1            – Sentinel-1 SLC via CNES PEPS (CNES account)
      RP2_GSA                     – Resurs-P hyperspectral (ROSCOSMOS account)

    For program sensors: S2 optical (full spectrum/glint) and L8 thermal are
    available anonymously.  CMEMS SSH/SST/ocean-colour products are *discovered*
    via the FEDEO series catalog (3256 entries) but must be downloaded from
    Copernicus Marine Service (marine.copernicus.eu, free account required).
    """

    BASE = 'https://fedeo.ceos.org'

    # Collections where enclosure links are public-S3 (no auth)
    PUBLIC_COLLECTIONS = {
        'EOP:SENTINEL-HUB:Sentinel2': {
            'sensor': 's2_l1c',
            'base_url': 'https://sentinel-s2-l1c.s3.amazonaws.com',
            'bands': ['B01.jp2', 'B02.jp2', 'B03.jp2', 'B04.jp2',
                      'B05.jp2', 'B06.jp2', 'B07.jp2', 'B08.jp2',
                      'B8A.jp2', 'B09.jp2', 'B10.jp2', 'B11.jp2', 'B12.jp2'],
        },
        'EOP:SENTINEL-HUB:Landsat8': {
            'sensor': 'l8_l1tp',
            'base_url': 'http://landsat-pds.s3.amazonaws.com',
            'bands': ['_B1.TIF', '_B2.TIF', '_B3.TIF', '_B4.TIF',
                      '_B5.TIF', '_B6.TIF', '_B7.TIF', '_B10.TIF', '_B11.TIF'],
        },
    }

    # Known CMEMS collections useful for WreckHunter (catalog-only through FEDEO;
    # download via marine.copernicus.eu with free CMEMS registration)
    CMEMS_COLLECTIONS = {
        'SEALEVEL_GLO_PHY_L4_NRT_008_046':
            'Global SSH L4 NRT 0.25° daily — SWOT-comparable altimetry',
        'SEALEVEL_GLO_PHY_L3_NRT_008_044':
            'Global along-track L3 SSH NRT — raw altimeter swaths',
        'SST_GLO_PHY_L4_NRT_010_043':
            'ODYSSEA Global SST L4 daily 0.01° — thermal detection',
        'OCEANCOLOUR_GLO_BGC_L4_NRT_009_102':
            'Global Ocean Colour L4 NRT — chlorophyll/turbidity/glint',
        'WIND_GLO_PHY_L4_NRT_012_004':
            'Global Ocean Wind L4 NRT — SAR coherence/glint wind correction',
        'SEAICE_GLO_PHY_L4_MY_011_020':
            'Global SAR Sea Ice Drift L4 multi-year — ice SAR baseline',
    }

    def __init__(self, dry_run: bool = False, output_dir: Optional[Path] = None):
        self.dry_run = dry_run
        self.output_dir = output_dir or (REPO / 'downloads' / 'fedeo')
        self.session = requests.Session()
        self.session.headers.update({
            'User-Agent': 'CESAROPS-WreckHunter2000/1.0',
            'Accept': 'application/geo+json',
        })

    # ------------------------------------------------------------------ search

    def search_collection(self, collection_id: str, bbox: List[float],
                          start: str, end: str,
                          max_results: int = 20) -> List[Dict]:
        """Query a FEDEO collection and return granule feature dicts."""
        lat_min, lon_min, lat_max, lon_max = bbox
        params = {
            'bbox': f'{lon_min},{lat_min},{lon_max},{lat_max}',
            'limit': max_results,
        }
        if start:
            params['startDate'] = f'{start}T00:00:00Z'
        if end:
            params['endDate'] = f'{end}T23:59:59Z'

        url = f'{self.BASE}/collections/{collection_id}/items'
        try:
            resp = self.session.get(url, params=params, timeout=40)
            resp.raise_for_status()
            j = resp.json()
            return j.get('features') or j.get('items') or []
        except Exception as exc:
            print(f'  ⚠ FEDEO search {collection_id}: {exc}')
            return []

    def _enclosure_links(self, item: Dict) -> List[str]:
        """Return all enclosure (data) hrefs from a FEDEO feature."""
        hrefs = []
        for lk in (item.get('links') or item.get('properties', {}).get('links') or []):
            if lk.get('rel') == 'enclosure':
                href = lk.get('href', '')
                if href.startswith('http') or href.startswith('s3://'):
                    hrefs.append(href)
        return hrefs

    # ---------------------------------------------------------------- download

    def _download_s2_tile(self, s3_path: str, dest_dir: Path) -> List[Path]:
        """Download individual band JP2 files from the public sentinel-s2-l1c bucket."""
        # s3_path looks like  s3://sentinel-s2-l1c/tiles/16/T/DM/2025/3/2/0
        # HTTP equivalent:    https://sentinel-s2-l1c.s3.amazonaws.com/tiles/16/T/DM/2025/3/2/0/
        key_path = s3_path.replace('s3://sentinel-s2-l1c', '').lstrip('/')
        base_http = f'https://sentinel-s2-l1c.s3.amazonaws.com/{key_path}'
        bands = self.PUBLIC_COLLECTIONS['EOP:SENTINEL-HUB:Sentinel2']['bands']
        dest_dir.mkdir(parents=True, exist_ok=True)
        downloaded = []
        for band in bands:
            url = f'{base_http}/{band}'
            dest = dest_dir / band
            if dest.exists():
                print(f'    [skip] {dest.name}')
                downloaded.append(dest)
                continue
            if self.dry_run:
                print(f'    [dry] {url}')
                continue
            r = self.session.get(url, stream=True, timeout=300)
            if r.status_code == 200:
                with open(dest, 'wb') as fh:
                    for chunk in r.iter_content(1 << 20):
                        fh.write(chunk)
                print(f'    {dest.name} ({dest.stat().st_size/1e6:.1f} MB)')
                downloaded.append(dest)
            else:
                print(f'    ⚠ {band} HTTP {r.status_code}')
        return downloaded

    def _download_l8_scene(self, enclosure_url: str, dest_dir: Path) -> List[Path]:
        """Download L8 band GeoTIFFs from the public landsat-pds S3 bucket."""
        # enclosure_url = http://landsat-pds.s3.amazonaws.com/c1/L8/023/031/<scene>/index.html
        scene_base = enclosure_url.replace('/index.html', '')
        # Infer scene_id from path  (last component)
        scene_id = scene_base.rstrip('/').split('/')[-1]
        bands = self.PUBLIC_COLLECTIONS['EOP:SENTINEL-HUB:Landsat8']['bands']
        dest_dir.mkdir(parents=True, exist_ok=True)
        downloaded = []
        for suffix in bands:
            url = f'{scene_base}/{scene_id}{suffix}'
            dest = dest_dir / f'{scene_id}{suffix}'
            if dest.exists():
                print(f'    [skip] {dest.name}')
                downloaded.append(dest)
                continue
            if self.dry_run:
                print(f'    [dry] {url}')
                continue
            r = self.session.get(url, stream=True, timeout=300)
            if r.status_code == 200:
                with open(dest, 'wb') as fh:
                    for chunk in r.iter_content(1 << 20):
                        fh.write(chunk)
                print(f'    {dest.name} ({dest.stat().st_size/1e6:.1f} MB)')
                downloaded.append(dest)
            else:
                print(f'    ⚠ {suffix} HTTP {r.status_code}')
        return downloaded

    # ------------------------------------------------------------- cmems info

    def list_cmems_collections(self) -> None:
        """Print CMEMS collection IDs useful for WreckHunter (SSH/SST/colour/wind)."""
        print('\n  CMEMS collections accessible via marine.copernicus.eu:')
        for cid, desc in self.CMEMS_COLLECTIONS.items():
            print(f'    {cid:45s} — {desc}')
        print('  (free registration at marine.copernicus.eu → `copernicusmarine` CLI)')

    # -------------------------------------------------------------------- run

    def run(self, bbox: List[float], start: str, end: str,
            max_results: int = 20) -> List[Path]:
        print(f"\n{'='*60}")
        print(f"FEDEO — Multi-sensor EO catalog")
        print(f"  BBOX:  {bbox}")
        print(f"  Range: {start} to {end}")
        print(f"{'='*60}")

        all_downloaded: List[Path] = []

        for coll_id, meta in self.PUBLIC_COLLECTIONS.items():
            print(f"\n[FEDEO] {coll_id}")
            items = self.search_collection(coll_id, bbox, start, end, max_results)
            print(f'  Found {len(items)} granules')

            for item in items:
                item_id = item.get('id', 'unknown')
                links = self._enclosure_links(item)
                if not links:
                    continue

                dest_subdir = self.output_dir / meta['sensor'] / item_id

                for href in links:
                    if href.startswith('s3://sentinel-s2-l1c'):
                        files = self._download_s2_tile(href, dest_subdir)
                        all_downloaded.extend(files)
                        break
                    elif 'landsat-pds.s3.amazonaws.com' in href and href.endswith('index.html'):
                        files = self._download_l8_scene(href, dest_subdir)
                        all_downloaded.extend(files)
                        break

        self.list_cmems_collections()

        print(f"\n  FEDEO total: {len(all_downloaded)} files")
        return all_downloaded


# ── Main orchestrator ─────────────────────────────────────────────────────

def list_sources():
    """Print available data sources."""
    print(f"\n{'='*70}")
    print("CESAROPS Universal Downloader — Available Sources")
    print(f"{'='*70}")
    sources = {
        'asf':            'ASF HyP3 — Sentinel-1 SAR (RTC/InSAR) — Free, Earthdata auth',
        'podaac':         'NASA PO.DAAC — SWOT SSH, ICESat-2 ATL13 — Free, Earthdata auth',
        'icesat2':        'ICESat-2 ATL03/ATL13 — Photon LiDAR bathymetry — Free, Earthdata auth',
        'usgs':           'USGS Earth Explorer — Landsat 8/9 — Free, API key required',
        'hls':            'NASA HLS — Harmonized Landsat Sentinel-2 — Free, Earthdata auth (AWS fallback)',
        'sentinel2_aws':  'AWS Sentinel-2 L2A COG — Element84 STAC — Free, no auth (10–60m)',
        'landsat_aws':    'AWS Landsat C2 L2 COG — Element84 STAC — Free, no auth (30m+thermal)',
        'modis':          'MODIS MOD11A1/MYD11A1 — 1km thermal daily — Free, Earthdata auth',
        'viirs':          'VIIRS VNP21A1D/VNP21A1N — 1km thermal day/night — Free, Earthdata auth',
        'noaa_coastwatch':'NOAA CoastWatch ERDDAP — Great Lakes GLSEA SST (~2024) — Free, no auth',
        'lidar':          'USGS 3DEP LiDAR — 1m point clouds, DEMs — Free, no auth',
        'fedeo':          'FEDEO CEOS catalog — S2 L1C + L8 L1TP via public S3; CMEMS SSH/SST/colour discovery',
    }
    for src, desc in sources.items():
        print(f"  {src:18s}  {desc}")
    print(f"\nSensor aliases:")
    print(f"  aws, optical, optical_aws  → sentinel2_aws + landsat_aws (FREE, no Earthdata — most optical/SWIR/thermal)")
    print(f"  sar, swot, icesat, hls, thermal, viirs, lidar, sst, sentinel2, landsat (explicit)")
    print(f"{'='*70}")


def list_cmr_stac_collections():
    """List high-value CMR-STAC LPCLOUD collections for wreck-hunt workflows."""
    client = CMRSTACClient('LPCLOUD')
    keyword_map = {
        'HLS/optical': 'HLS',
        'MODIS thermal': 'MODIS',
        'VIIRS thermal': 'VIIRS',
        'ECOSTRESS thermal': 'ECOSTRESS',
        'GEDI lidar': 'GEDI',
        'SMAP soil moisture': 'SMAP',
    }
    print(f"\n{'='*70}")
    print('CMR-STAC LPCLOUD — Useful Collections')
    print(f"{'='*70}")
    for label, query in keyword_map.items():
        cols = client.search_collections(query, limit=12)
        print(f"\n{label} (q={query}):")
        if not cols:
            print('  (no matches)')
            continue
        for c in cols[:8]:
            print(f"  {c.get('id', ''):24s}  {c.get('title', '')[:70]}")
    print(f"\nProvider root: {client.base}")
    print(f"{'='*70}")

def main():
    parser = argparse.ArgumentParser(description='CESAROPS Universal Satellite Data Downloader')
    parser.add_argument('--area', type=str, help='Preset area name (see --list-areas)')
    parser.add_argument('--bbox', type=str, help='Custom bbox: lat_min,lon_min,lat_max,lon_max')
    parser.add_argument('--dates', type=str, nargs=2, help='Date range: START END (YYYY-MM-DD)')
    parser.add_argument('--sensors', type=str, default='all',
                        help='Comma-separated: sar,optical,swot,icesat,icesat2,landsat,hls,thermal,modis,lidar,sst,all')
    parser.add_argument('--max-results', type=int, default=20, help='Max results per source')
    parser.add_argument('--dry-run', action='store_true', help='Show what would be downloaded')
    parser.add_argument('--list-sources', action='store_true', help='List available data sources')
    parser.add_argument('--list-cmr-stac', action='store_true', help='List useful CMR-STAC LPCLOUD collections')
    parser.add_argument('--list-areas', action='store_true', help='List preset areas')
    parser.add_argument('--output', type=str, help='Override output directory')
    parser.add_argument('--no-rtc', action='store_true', help='Download raw SLC instead of RTC for SAR')
    args = parser.parse_args()

    if args.list_sources:
        list_sources()
        return

    if args.list_cmr_stac:
        list_cmr_stac_collections()
        return

    if args.list_areas:
        print("\nPreset areas:")
        for name, info in AREAS.items():
            print(f"  {name:25s}  {info['label']}  [{info['bbox']}]")
        return

    # Resolve bbox
    if args.area:
        if args.area not in AREAS:
            print(f"Unknown area: {args.area}")
            print("Available areas:")
            for name in AREAS:
                print(f"  {name}")
            return
        bbox = AREAS[args.area]['bbox']
        print(f"Area: {AREAS[args.area]['label']}")
    elif args.bbox:
        parts = [float(x) for x in args.bbox.split(',')]
        bbox = parts
    else:
        bbox = AREAS['straits_of_mackinac']['bbox']
        print(f"Default area: Straits of Mackinac")

    # Resolve dates
    if args.dates:
        start, end = args.dates
    else:
        # Default: last 6 months of available data
        end = datetime.now().strftime('%Y-%m-%d')
        start = (datetime.now() - timedelta(days=180)).strftime('%Y-%m-%d')

    # Resolve sensors (aliases may expand to comma-separated source lists)
    sensor_map = {
        'sar': 'asf',
        'optical': 'sentinel2_aws,landsat_aws',
        # Free AWS Open Data via Element84 STAC — no Earthdata token (covers most bands)
        'aws': 'sentinel2_aws,landsat_aws',
        'optical_aws': 'sentinel2_aws,landsat_aws',
        'stac': 'sentinel2_aws,landsat_aws',
        'swot': 'podaac',
        'icesat': 'podaac',
        'icesat2': 'podaac',
        # Default Landsat to the authed USGS M2M path — the AWS usgs-landsat COG
        # bucket is requester-pays and 403s without credentials. Use the
        # explicit 'landsat_aws' token to force the no-auth AWS COG path.
        'landsat': 'usgs',
        'landsat_aws': 'landsat_aws',
        'sentinel2': 'sentinel2_aws',
        'sentinel2_aws': 'sentinel2_aws',
        'hls': 'hls',
        'thermal': 'modis,viirs',
        'modis': 'modis',
        'viirs': 'viirs',
        'lidar': 'lidar',
        'sst': 'noaa_coastwatch',
        'coastwatch': 'noaa_coastwatch',
        'fedeo': 'fedeo',
        'cmems': 'fedeo',
        'ceos': 'fedeo',
    }
    if args.sensors == 'all':
        sources = ['asf', 'podaac', 'usgs', 'hls',
                   'sentinel2_aws', 'landsat_aws', 'fedeo',
                   'modis', 'viirs', 'noaa_coastwatch', 'lidar']
    else:
        sources = []
        for token in args.sensors.split(','):
            mapped = sensor_map.get(token.strip(), token.strip())
            sources.extend(x.strip() for x in mapped.split(',') if x.strip())
        # De-duplicate while preserving order. Several aliases collapse onto the
        # same backend (e.g. swot + icesat2 → podaac, which fetches both datasets
        # in one run), so without this PODAAC/etc. would run multiple times.
        _seen = set()
        sources = [s for s in sources if not (s in _seen or _seen.add(s))]

    output_base = Path(args.output) if args.output else (REPO / 'downloads')

    all_downloaded = []
    for src in sources:
        try:
            if src == 'asf':
                dl = ASFDownloader(dry_run=args.dry_run, output_dir=output_base / 'sar')
                result = dl.run(bbox, start, end, max_granules=args.max_results, process_rtc=not args.no_rtc)
                all_downloaded.extend(result)
            elif src == 'podaac':
                dl = PODAACDownloader(dry_run=args.dry_run, output_dir=output_base / 'podaac')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'usgs':
                dl = USGSDownloader(dry_run=args.dry_run, output_dir=output_base / 'usgs')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'hls':
                dl = HLSDownloader(dry_run=args.dry_run, output_dir=output_base / 'hls')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'modis':
                dl = MODISDownloader(dry_run=args.dry_run, output_dir=output_base / 'modis')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'viirs':
                dl = MODISDownloader(dry_run=args.dry_run, output_dir=output_base / 'viirs')
                result = dl.run(
                    bbox,
                    start,
                    end,
                    products=['vnp21a1d', 'vnp21a1n'],
                    max_results=args.max_results,
                )
                all_downloaded.extend(result)
            elif src == 'noaa_coastwatch':
                dl = NOAACoastwatchDownloader(dry_run=args.dry_run, output_dir=output_base / 'noaa_coastwatch')
                result = dl.run(bbox, start, end)
                all_downloaded.extend(result)
            elif src == 'lidar':
                dl = USGSLiDARDownloader(dry_run=args.dry_run, output_dir=output_base / 'lidar')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'sentinel2_aws':
                dl = AWSSentinel2Downloader(dry_run=args.dry_run, output_dir=output_base / 'sentinel2_aws')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'landsat_aws':
                dl = AWSLandsatDownloader(dry_run=args.dry_run, output_dir=output_base / 'landsat_aws')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'icesat2':
                dl = ICESat2Downloader(dry_run=args.dry_run, output_dir=output_base / 'icesat2')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            elif src == 'fedeo':
                dl = FEDEODownloader(dry_run=args.dry_run, output_dir=output_base / 'fedeo')
                result = dl.run(bbox, start, end, max_results=args.max_results)
                all_downloaded.extend(result)
            else:
                print(f"  Unknown source: {src}")
        except Exception as e:
            print(f"  ⚠ {src} failed: {e}")

    print(f"\n{'='*70}")
    print(f"DOWNLOAD SUMMARY")
    print(f"{'='*70}")
    print(f"  Total files: {len(all_downloaded)}")
    total_size = sum(f.stat().st_size for f in all_downloaded if f.exists())
    print(f"  Total size: {total_size / 1e6:.1f} MB")
    print(f"  Output: {output_base}")
    print(f"{'='*70}")

if __name__ == '__main__':
    main()
