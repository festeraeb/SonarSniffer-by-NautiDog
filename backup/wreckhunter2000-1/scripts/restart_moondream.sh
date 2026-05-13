#!/bin/bash
pkill -f moondream_c3 2>/dev/null
sleep 2
export VALIDATOR_PORT=5572
nohup python3 ~/moondream_c3.py > ~/moondream.log 2>&1 &
echo "Moondream2 started PID=$!"
