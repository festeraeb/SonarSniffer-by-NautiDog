#!/usr/bin/env python3
import json

d = json.load(open('/tmp/credential_inventory.json'))
print(f'Total env vars referenced: {len(d["environment_variables_referenced"])}')
print(f'Credential files found: {len(d["credential_files_found"])}')
for f in d['credential_files_found']:
    print(f'  {f}')
print('\nLikely secrets (KEY/SECRET/TOKEN/PASS/AUTH in name):')
secrets = [v for v in d['environment_variables_referenced'] if any(k in v for k in ['KEY','SECRET','TOKEN','PASS','AUTH'])]
for s in sorted(secrets):
    print(f'  {s}')
print(f'\nTotal likely secrets: {len(secrets)}')
