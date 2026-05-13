"""Quick test: get LP DAAC S3 credentials via Bearer JWT, list bucket."""
import os
import sys
import requests
import boto3

env_path = '/home/cesarops/wreckhunter2000-1/.env'
token = None
with open(env_path) as f:
    for line in f:
        line = line.strip()
        if line.startswith('EARTHDATA_TOKEN='):
            token = line.split('=', 1)[1].strip('"\'')
            break

if not token:
    print('ERROR: EARTHDATA_TOKEN not found in .env')
    sys.exit(1)

print(f'Token prefix: {token[:20]}...')

hdrs = {'Authorization': f'Bearer {token}'}
r = requests.get('https://data.lpdaac.earthdatacloud.nasa.gov/s3credentials',
                 headers=hdrs, timeout=20)
print(f'S3CREDS HTTP status: {r.status_code}')
if r.status_code != 200:
    print(r.text[:400])
    sys.exit(1)

c = r.json()
print(f'S3CREDS keys: {list(c.keys())}')
print(f'Expiration: {c.get("expiration")}')

s3 = boto3.client(
    's3',
    aws_access_key_id=c['accessKeyId'],
    aws_secret_access_key=c['secretAccessKey'],
    aws_session_token=c['sessionToken'],
    region_name='us-west-2',
)

try:
    resp = s3.list_objects_v2(Bucket='lp-prod-protected', Prefix='HLSL30.020/', MaxKeys=3)
    keys = [o['Key'] for o in resp.get('Contents', [])]
    print(f'Sample keys: {keys}')
    print('BUCKET ACCESS OK')
except Exception as e:
    print(f'Bucket list error: {e}')
    sys.exit(1)
