"""Check Pi disk usage and SWIR file counts."""
import sys, os
sys.stdout.reconfigure(encoding="utf-8")
sys.path.insert(0, os.path.dirname(__file__))
from remote_dispatch import SSHNode, PI_TAILSCALE, PI_USER, PI_PASS, PI_KEY
node = SSHNode(host=PI_TAILSCALE, user=PI_USER, password=PI_PASS, key_path=PI_KEY)
node.ping()
r = node.run("du -sh /home/pi/cesarops/downloads/jobs/*/hls/ 2>/dev/null")
print("Site sizes:", r["stdout"].strip())

r2 = node.run(
    "find /home/pi/cesarops/downloads/jobs -name '*.B11.tif' -o "
    "-name '*.B12.tif' -o -name '*.B06.tif' -o -name '*.B07.tif' | wc -l"
)
print("SWIR band files (B06/B07/B11/B12):", r2["stdout"].strip())

r3 = node.run(
    "find /home/pi/cesarops/downloads/jobs -name '*.B11.tif' -o "
    "-name '*.B12.tif' -o -name '*.B06.tif' -o -name '*.B07.tif' | "
    "xargs du -sh 2>/dev/null | tail -3"
)
print("Sample SWIR file sizes:")
print(r3["stdout"].strip()[:300])
node.close()
