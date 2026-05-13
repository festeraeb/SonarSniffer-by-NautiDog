import urllib.request, json

PI = 'http://100.127.66.32:8099'

r = urllib.request.urlopen(f'{PI}/workers/online', timeout=8)
data = json.loads(r.read())
print('/workers/online:', data)

body = json.dumps({'worker_id':'test_cpu', 'has_gpu': False, 'has_tpu': False}).encode()
req = urllib.request.Request(f'{PI}/jobs/claim', data=body, method='POST', headers={'Content-Type':'application/json'})
r = urllib.request.urlopen(req, timeout=8)
data = json.loads(r.read())
print('/jobs/claim (cpu-only worker):', data)

body = json.dumps({'worker_id':'test_gpu', 'has_gpu': True, 'has_tpu': False, 'vram_gb': 8}).encode()
req = urllib.request.Request(f'{PI}/jobs/claim', data=body, method='POST', headers={'Content-Type':'application/json'})
r = urllib.request.urlopen(req, timeout=8)
data = json.loads(r.read())
job = data.get('job')
print('/jobs/claim (gpu worker):', 'got job' if job else 'no job', '|', job['id'][:8] if job else '', job.get('job_type','') if job else '')

if job:
    body = json.dumps({'success': False, 'error_msg': 'smoke test'}).encode()
    req = urllib.request.Request(f"{PI}/jobs/{job['id']}/finish", data=body, method='POST', headers={'Content-Type':'application/json'})
    r = urllib.request.urlopen(req, timeout=8)
    print('  unclaimed (FAILED):', json.loads(r.read()))
