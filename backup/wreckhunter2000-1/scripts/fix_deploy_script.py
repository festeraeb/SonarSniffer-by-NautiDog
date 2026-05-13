#!/usr/bin/env python3
import pathlib
p = pathlib.Path("/tmp/build_tauri_and_deploy.py")
t = p.read_text()
t = t.replace(
    'deploy_web = open("/home/cesarops/wreckhunter2000-1/scripts/deploy_web.py").read()',
    'deploy_web = "Deploys tauri/dist-web/ to IONOS via SFTP using paramiko. Run: python scripts/deploy_web.py --ionos"'
)
t = t.replace(
    "print(f\"Loaded deploy_web.py: {len(deploy_web)} chars\")",
    "print(f\"deploy_web reference loaded\")"
)
p.write_text(t)
print("Fixed")
