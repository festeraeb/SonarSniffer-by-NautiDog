#!/bin/bash
source /home/cesarops/.cargo/env
CAKE=/home/cesarops/benchmark/cake/target/release/cake
echo "Starting Cake 1.5B benchmark..."
$CAKE run Qwen/Qwen2.5-1.5B-Instruct "Shipwreck-detection-using-satellite-imagery-involves" --sample-len 128 --temperature 0.7 2>&1
echo "EXIT=$?"
