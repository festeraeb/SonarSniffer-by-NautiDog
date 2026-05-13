#!/bin/bash
# deploy_scan_worker.sh
# Push scan_worker.py + scan_queue.py to i7 and install the systemd service.
# Run once from this machine; SSH key must already be in authorized_keys.
#
# Usage:
#   bash scripts/deploy_scan_worker.sh
#   bash scripts/deploy_scan_worker.sh --restart-api  # also reload wrecks-api

set -e

I7="cesarops@68.55.42.202"
REMOTE="/home/cesarops/wreckhunter2000-1"
VENV="/home/cesarops/tpu-venv"

echo "=== Deploying scan_worker to i7 ==="

# 1. Push changed files
echo "--- Syncing files ---"
scp scan_queue.py          "$I7:$REMOTE/scan_queue.py"
scp scan_worker.py         "$I7:$REMOTE/scan_worker.py"
scp wrecks_api/app.py      "$I7:$REMOTE/wrecks_api/app.py"
scp scripts/scan-worker.service "$I7:/tmp/scan-worker.service"

# 2. Install paramiko for IONOS SFTP (non-fatal if it fails)
echo "--- Installing paramiko ---"
ssh "$I7" "$VENV/bin/pip install paramiko --quiet" || true

# 3. Init the DB (creates db/scan_queue.db if absent)
echo "--- Initializing scan queue DB ---"
ssh "$I7" "cd $REMOTE && $VENV/bin/python -c 'import scan_queue; scan_queue.init_db(); print(\"Queue DB ready\")'"

# 4. Install systemd service
echo "--- Installing systemd service ---"
ssh "$I7" "sudo mv /tmp/scan-worker.service /etc/systemd/system/scan-worker.service"
ssh "$I7" "sudo systemctl daemon-reload"
ssh "$I7" "sudo systemctl enable scan-worker.service"
ssh "$I7" "sudo systemctl restart scan-worker.service"
ssh "$I7" "sudo systemctl status scan-worker.service --no-pager"

# 5. Optionally reload wrecks-api so new /scan/queue endpoints are live
if [[ "$1" == "--restart-api" ]]; then
  echo "--- Restarting wrecks-api ---"
  ssh "$I7" "sudo systemctl restart wrecks-api.service"
  ssh "$I7" "sudo systemctl status wrecks-api.service --no-pager"
fi

echo ""
echo "=== Deployed. Check the worker ==="
echo "  Live log:  ssh $I7 'journalctl -u scan-worker -f'"
echo "  API depth: curl http://68.55.42.202/scan/queue/depth"
echo "  Queue:     curl http://68.55.42.202/scan/queue"
echo ""
echo "To push an urgent scan from anywhere:"
echo "  curl -X POST http://68.55.42.202/scan/queue \\"
echo "       -H 'Content-Type: application/json' \\"
echo "       -d '{\"label\":\"andaste_urgent\",\"bbox\":[45.0,-87.0,45.5,-86.5],\"sensors\":[\"thermal\",\"optical\"],\"priority\":0}'"
