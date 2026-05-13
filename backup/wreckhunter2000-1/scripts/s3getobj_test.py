"""Get one real CMR granule URL for Lake Erie Oct 2015, then test S3 GetObject download."""
import requests
import boto3
import os
import sys

env_path = '/home/cesarops/wreckhunter2000-1/.env'
token = None
with open(env_path) as fh:
    for line in fh:
        line = line.strip()
        if line.startswith('EARTHDATA_TOKEN='):
            token = line.split('=', 1)[1].strip('"\'')
            break

# CMR search (no auth — public endpoint)
cmr_url = 'https://cmr.earthdata.nasa.gov/search/granules.json'
params = {
    'short_name': 'HLSL30',
    'version': '2.0',
    'temporal': '2015-10-01T00:00:00Z,2015-10-31T23:59:59Z',
    'bounding_box': '-83.5,41.3,-78.8,42.5',
    'page_size': 1,
    'page_num': 1,
}
r = requests.get(cmr_url, params=params, timeout=30)
print(f'CMR status: {r.status_code}')
entries = r.json().get('feed', {}).get('entry', [])
if not entries:
    print('No CMR results — cannot test download')
    sys.exit(1)

entry = entries[0]
print(f'Granule: {entry.get("producer_granule_id","?")}')

# Find an LP DAAC HTTPS link
href = None
for link in entry.get('links', []):
    h = link.get('href', '')
    if 'lp-prod-protected' in h and h.endswith('.tif'):
        href = h
        break
    if h.startswith('https://data.lpdaac.earthdatacloud.nasa.gov') and h.endswith('.tif'):
        href = h
        break

if not href:
    for link in entry.get('links', []):
        h = link.get('href', '')
        if h.startswith('https://data.lpdaac.earthdatacloud.nasa.gov'):
            href = h
            break

if not href:
    print('No LP DAAC link found; links:', [l.get('href','') for l in entry.get('links',[])][:5])
    sys.exit(1)

print(f'URL: {href}')

# Extract S3 key
if '/lp-prod-protected/' in href:
    s3_key = href.split('/lp-prod-protected/', 1)[1]
elif href.startswith('/lp-prod-protected/'):
    s3_key = href[len('/lp-prod-protected/'):]
else:
    s3_key = None

print(f'S3 key: {s3_key}')

if not s3_key:
    print('Cannot extract S3 key from URL')
    sys.exit(1)

# Get S3 credentials
hdrs = {'Authorization': f'Bearer {token}'}
cr = requests.get('https://data.lpdaac.earthdatacloud.nasa.gov/s3credentials',
                   headers=hdrs, timeout=20)
c = cr.json()
s3 = boto3.client(
    's3',
    aws_access_key_id=c['accessKeyId'],
    aws_secret_access_key=c['secretAccessKey'],
    aws_session_token=c['sessionToken'],
    region_name='us-west-2',
)

dest = '/tmp/test_hls_tile.tif'
print(f'Attempting s3://lp-prod-protected/{s3_key} → {dest}')
try:
    s3.download_file('lp-prod-protected', s3_key, dest)
    sz = os.path.getsize(dest)
    print(f'SUCCESS: {dest} ({sz/1e6:.2f} MB)')
except Exception as e:
    print(f'FAILED: {e}')
    sys.exit(1)
