#!/usr/bin/env python3
"""
Ask Qwen3.6-35B to:
1. Create a Tauri wrapper for Mission Control (Windows desktop app)
2. Add Mission Control to the existing wh2000 web frontend
3. Deploy via the IONOS script
"""
import json
import urllib.request
import os

NAUTIVECS = "http://localhost:5003"
KOBOLD = "http://localhost:5001/v1"
OUTPUT = "/home/cesarops/wreckhunter2000-1/docs/tauri_deploy_impl.md"

# Get context about existing tauri setup and deploy_web
print("=== Querying nautivecs ===")
queries = [
    "tauri vite config react build web mode",
    "deploy web IONOS SFTP upload dist",
]
all_context = []
for q in queries:
    payload = json.dumps({"query": q, "top_k": 3, "include_context": True}).encode()
    req = urllib.request.Request(f"{NAUTIVECS}/query", data=payload, headers={"Content-Type": "application/json"}, method="POST")
    try:
        resp = json.loads(urllib.request.urlopen(req, timeout=30).read())
        if resp.get("context_block"):
            all_context.append(resp["context_block"])
        print(f"  [{q[:50]}] -> {len(resp['results'])} results")
    except Exception as e:
        print(f"  [{q[:50]}] -> ERROR: {e}")

combined_context = "\n\n".join(all_context)

# Load the mission control HTML from the running server
import subprocess
mc_html = subprocess.run(
    ["curl", "-s", "http://localhost:3000/"],
    capture_output=True, text=True, timeout=10
).stdout
print(f"Loaded Mission Control HTML: {len(mc_html)} chars")

# Load deploy_web.py for reference
deploy_web = open("/home/cesarops/wreckhunter2000-1/scripts/deploy_web.py").read()
print(f"Loaded deploy_web.py: {len(deploy_web)} chars")

print("\n=== Generating Tauri + Web Deploy ===")

system_msg = f"""You are CESAROPS (Qwen3.6-35B). You just deployed Mission Control as a web server on port 3000.

The user wants:
1. A Tauri desktop app wrapper so he can run it locally on Windows (connects to T440 backend)
2. The Mission Control UI added to the existing wh2000 web frontend on IONOS hosting
3. Deployed using the existing deploy_web.py --ionos script

The existing Mission Control HTML (currently served by axum on :3000):
```html
{mc_html[:6000]}
```

The existing deploy_web.py script:
```python
{deploy_web[:3000]}
```

The existing tauri project is at `tauri/` with vite + react. But for Mission Control we want a STANDALONE page, not integrated into the React app. It should be a separate HTML file deployed alongside the existing wh2000 app.

Codebase context:
{combined_context[:3000]}

IMPORTANT: The user is on Windows. Tauri commands must work in PowerShell."""

user_msg = """Create TWO things:

## 1. Tauri Desktop Wrapper (Windows)

A minimal Tauri app that:
- Opens a window pointing at the Mission Control server (http://100.72.182.77:3000 on LAN, or https://app.cesarops.org when remote)
- Auto-detects if on local network (try LAN first, fall back to tunnel)
- Window title: "CESAROPS Mission Control"
- 1200x800 default size, resizable
- System tray icon (optional)

Output:
=== FILE: tauri-mission-control/package.json ===
=== FILE: tauri-mission-control/src-tauri/tauri.conf.json ===
=== FILE: tauri-mission-control/src-tauri/src/main.rs ===
=== FILE: tauri-mission-control/index.html ===
=== FILE: tauri-mission-control/setup.ps1 ===
[PowerShell script to: npm init, install tauri deps, build]

## 2. Web Deployment to IONOS

Take the Mission Control HTML and make it a standalone page at /wh2000/mission-control/index.html on IONOS hosting. It should:
- Work standalone (no React, no build step)
- Connect to https://api.cesarops.org for the backend (or configurable)
- Be deployable via the existing `python scripts/deploy_web.py --ionos` (just needs the file in dist-web/)

Output:
=== FILE: tauri/dist-web/mission-control/index.html ===
[The full standalone Mission Control page, with API URLs pointing to cesarops.org tunnel]

=== FILE: scripts/deploy_mission_control.ps1 ===
[PowerShell script that: copies the HTML to dist-web, runs deploy_web.py --ionos]

Keep it simple. The Tauri app is just a WebView wrapper. The web deploy is just copying one HTML file."""

request_body = {
    "model": "koboldcpp/Qwen3.6-35B-A3B-MXFP4_MOE",
    "messages": [
        {"role": "system", "content": system_msg},
        {"role": "user", "content": user_msg}
    ],
    "max_tokens": 16384,
    "temperature": 0.4,
    "top_p": 0.9,
    "presence_penalty": 0.8
}

print(f"System: {len(system_msg)} chars | User: {len(user_msg)} chars")
print("Generating...")

payload = json.dumps(request_body).encode()
req = urllib.request.Request(f"{KOBOLD}/chat/completions", data=payload, headers={"Content-Type": "application/json"}, method="POST")

try:
    resp = json.loads(urllib.request.urlopen(req, timeout=1800).read())
    content = resp["choices"][0]["message"]["content"]

    with open(OUTPUT, "w") as f:
        f.write(content)

    print(f"\nGenerated: {len(content)} chars")

    # Parse files
    import re
    file_pattern = r'=== FILE: (.+?) ==='
    parts = re.split(file_pattern, content)

    files_written = 0
    if len(parts) > 1:
        for i in range(1, len(parts), 2):
            filename = parts[i].strip()
            file_content = parts[i+1].strip() if i+1 < len(parts) else ""
            file_content = re.sub(r'^```\w*\n?', '', file_content)
            file_content = re.sub(r'\n?```\s*$', '', file_content)
            file_content = file_content.strip()

            filepath = f"/home/cesarops/wreckhunter2000-1/{filename}"
            os.makedirs(os.path.dirname(filepath), exist_ok=True)
            with open(filepath, "w") as f:
                f.write(file_content + "\n")
            print(f"  Written: {filename} ({len(file_content)} bytes)")
            files_written += 1

    print(f"\nFiles: {files_written}")

except Exception as e:
    print(f"ERROR: {e}")
    import traceback
    traceback.print_exc()
