#!/bin/bash
export PATH=$PATH:~/.local/bin
export KOBOLD_BASE_URL=http://localhost:5555/v1
export NAUTIVECS_URL=http://100.72.182.77:5003
export CESAROPS_API_URL=http://100.72.182.77:5001/v1
pkill -f thought_engine.py 2>/dev/null
sleep 2
nohup python3 ~/thought_engine.py > /tmp/thought_engine.log 2>&1 &
sleep 5
curl -s http://localhost:5556/health
