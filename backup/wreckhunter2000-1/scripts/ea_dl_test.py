"""Test earthaccess authenticated download of one LP DAAC HLS file."""
import os, sys, requests

# Load .env
env_path = '/home/cesarops/wreckhunter2000-1/.env'
with open(env_path) as fh:
    for line in fh:
        line = line.strip()
        if '=' in line and not line.startswith('#'):
            k, v = line.split('=', 1)
            os.environ[k] = v.strip('"\'')

# Get one real CMR URL
cmr = 'https://cmr.earthdata.nasa.gov/search/granules.json'
params = {
    'short_name': 'HLSL30', 'version': '2.0',
    'temporal': '2015-10-01T00:00:00Z,2015-10-31T23:59:59Z',
    'bounding_box': '-83.5,41.3,-78.8,42.5',
    'page_size': 1,
}
r = requests.get(cmr, params=params, timeout=20)
entry = r.json()['feed']['entry'][0]
print(f'Granule: {entry.get("producer_granule_id","?")}')
href = None
for link in entry.get('links', []):
    h = link.get('href', '')
    if 'lp-prod-protected' in h and h.endswith('.tif'):
        href = h
        break
print(f'URL: {href}')
if not href:
    print('No LP DAAC .tif URL found'); sys.exit(1)

# Test earthaccess session
import earthaccess
print(f'earthaccess version: {earthaccess.__version__}')
auth = earthaccess.login(strategy='environment')
print(f'Login: {auth}')

session = None
if hasattr(earthaccess, 'get_requests_https_session'):
    session = earthaccess.get_requests_https_session()
    print('Using get_requests_https_session()')
elif hasattr(auth, 'get_session'):
    session = auth.get_session()
    print('Using auth.get_session()')
else:
    print('No session method found; trying direct download')

if session is not None:
    resp = session.get(href, stream=True, timeout=120)
    print(f'Response: HTTP {resp.status_code}')
    if resp.status_code == 200:
        dest = '/tmp/ea_test.tif'
        with open(dest, 'wb') as f:
            for chunk in resp.iter_content(1 << 20):
                f.write(chunk)
        sz = os.path.getsize(dest)
        print(f'SUCCESS: {dest} ({sz/1e6:.2f} MB)')
    else:
        print(f'FAILED: {resp.status_code} {resp.text[:200]}')
        sys.exit(1)
else:
    # Use earthaccess.download directly
    results = earthaccess.search_data(short_name='HLSL30', version='2.0',
        temporal=('2015-10-01', '2015-10-31'),
        bounding_box=(-83.5, 41.3, -78.8, 42.5),
        count=1)
    print(f'Search results: {len(results)}')
    if results:
        files = earthaccess.download(results[:1], '/tmp/')
        print(f'Downloaded: {files}')
