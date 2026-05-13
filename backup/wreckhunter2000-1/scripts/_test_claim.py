import urllib.request, json, time, sys
time.sleep(2)

PI = "http://100.127.66.32:8099"

def api(method, path, body=None):
    req = urllib.request.Request(
        PI + path, method=method,
        data=json.dumps(body).encode() if body else None,
        headers={"Content-Type": "application/json"} if body else {},
    )
    with urllib.request.urlopen(req, timeout=8) as r:
        return json.loads(r.read())

# Test claim
resp = api("POST", "/jobs/claim", {"worker_id": "test_node", "has_gpu": True, "has_tpu": False, "vram_gb": 4})
job = resp.get("job")
if job:
    jid = job["id"]
    print("CLAIMED:", jid[:8], job["label"])
    # Reset it back to QUEUED manually via sqlite (the finish endpoint marks DONE/FAILED)
    # Just leave it FAILED for now — user can reset with _reset_running.py
    fin = api("POST", f"/jobs/{jid}/finish", {"success": False, "error_msg": "endpoint_test"})
    print("Finished:", fin)
else:
    print("No job available (no GPU-capable job found or queue empty)")

# Quick summary
s = api("GET", "/workers")
print("Summary:", s.get("summary"))
