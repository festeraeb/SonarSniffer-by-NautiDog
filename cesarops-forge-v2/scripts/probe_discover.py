#!/usr/bin/env python3
import json, sys, urllib.request
data = json.loads(urllib.request.urlopen("http://127.0.0.1:9100/cluster/discover", timeout=8).read())
for n in data:
    ports = ', '.join(f"{p['port']}={'UP' if p['online'] else 'down'}" for p in n['ports'])
    reach = str(n['online'])
    svc = str(n['services_online'])
    print(f"{n['name']:38s} reach={reach:5s} svc={svc:5s}  {ports}")
