import json, os, sys
path = 'outputs/FULL_BASIN_SCAN_REPORT.json'
if not os.path.exists(path):
    print('report not found:', path)
    sys.exit(1)
with open(path, 'r', encoding='utf-8') as f:
    d = json.load(f)

candidates = d.get('all_detections') or d.get('detections') or d.get('candidates') or []
out = []
for e in candidates:
    if not isinstance(e, dict):
        continue
    label = ' '.join([str(e.get(k, '')).lower() for k in ('class','type','label','signature')])
    if 'construction_barge' in label or 'bridgebuilder' in label or 'construction barge' in label:
        coords = e.get('coord') or e.get('coords') or e.get('location') or e.get('wgs84') or e.get('position') or e.get('center')
        out.append({'id': e.get('id'), 'label': label, 'coords': coords, 'raw': e})

if not out:
    print('NOT FOUND: no construction_barge candidate')
    sys.exit(0)

print('FOUND', len(out), 'construction_barge candidate(s)')
for i, item in enumerate(out, 1):
    print('---', i)
    print('id:', item['id'])
    print('label:', item['label'])
    print('coords:', item['coords'])
    if isinstance(item['coords'], dict):
        print('lat:', item['coords'].get('lat') or item['coords'].get('latitude'))
        print('lon:', item['coords'].get('lon') or item['coords'].get('longitude'))
    elif isinstance(item['coords'], (list, tuple)) and len(item['coords']) >= 2:
        print('lon,lat:', item['coords'][0], item['coords'][1])

