#!/usr/bin/env python3
"""
CESAROPS3 DEPLOYMENT HELPER
Fixes BIOS GPU issue and gets koboldcpp running on Xeon

Step 1: Diagnose current GPU state
Step 2: Install/update NVIDIA drivers if needed
Step 3: Launch koboldcpp with optimal GPU settings
Step 4: Test LLM connectivity

Usage:
    python3 deploy_cesarops3.py [--host 10.0.0.56] [--user cesarops] [--action diagnose|install|launch|test]
"""

import subprocess
import sys
import argparse
from pathlib import Path

XENON_HOST = "10.0.0.56"
XENON_USER = "cesarops"

def ssh_run(host, user, cmd, verbose=False):
    """SSH execute with output"""
    try:
        if verbose:
            print(f"  $ ssh {user}@{host} '{cmd[:60]}...'")
        result = subprocess.run(
            f'ssh -o ConnectTimeout=10 {user}@{host} "{cmd}"',
            shell=True,
            capture_output=True,
            text=True,
            timeout=60
        )
        return result.returncode == 0, result.stdout.strip(), result.stderr.strip()
    except Exception as e:
        return False, "", str(e)

def action_diagnose(host, user):
    """Run GPU diagnostics"""
    print("╔" + "═"*78 + "╗")
    print("║ STEP 1: GPU DIAGNOSTICS                                                     ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    # nvidia-smi
    print("Checking NVIDIA driver...")
    success, out, err = ssh_run(host, user, "nvidia-smi -L", verbose=True)
    if success:
        gpu_count = len(out.strip().split('\n'))
        print(f"  ✓ {gpu_count} GPU(s) detected:")
        for line in out.strip().split('\n'):
            print(f"    └─ {line}")
    else:
        print(f"  ✗ Driver check failed: {err}")
        return False
    
    print()
    
    # CUDA enumeration
    print("Checking CUDA/CuPy enumeration...")
    success, out, err = ssh_run(host, user, 
        "python3 -c \"import cupy as cp; print(f'CUDA Devices: {cp.cuda.runtime.getDeviceCount()}')\"", 
        verbose=True)
    if success:
        print(f"  ✓ {out}")
    else:
        print(f"  ✗ CuPy error: {err}")
        print("    → You may need to reinstall CuPy or check CUDA installation")
    
    print()
    return gpu_count >= 1

def action_install_drivers(host, user):
    """Install/update NVIDIA drivers"""
    print("╔" + "═"*78 + "╗")
    print("║ STEP 2: NVIDIA DRIVER INSTALLATION                                          ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    print("This would install the latest NVIDIA drivers on the Xeon.")
    print("Run this manually on cesarops3:")
    print()
    print("  sudo apt-get update")
    print("  sudo apt-get install -y nvidia-driver-555  # or latest available")
    print("  sudo reboot")
    print()
    print("Then verify with: nvidia-smi -L")
    print()

def action_launch_koboldcpp(host, user, model_path, port):
    """Launch koboldcpp via SSH"""
    print("╔" + "═"*78 + "╗")
    print("║ STEP 3: LAUNCHING KOBOLDCPP                                                 ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    print(f"Model: {model_path}")
    print(f"Port: {port}")
    print()
    
    # Generate launcher command
    print("Generating koboldcpp launch command...")
    
    # Check if model exists
    success, out, err = ssh_run(host, user, f"ls -lh {model_path}", verbose=True)
    if success:
        print(f"  ✓ Model file found: {out}")
    else:
        print(f"  ✗ Model file not found: {model_path}")
        print(f"    Error: {err}")
        return False
    
    print()
    print("Launching koboldcpp (this will run in background)...")
    print()
    
    # Launch koboldcpp in background with nohup
    launch_cmd = f"""
cd ~ && nohup python3 /home/cesarops/launch_koboldcpp.py \\
  --model {model_path} \\
  --port {port} \\
  > ~/koboldcpp.log 2>&1 &
sleep 2
echo 'KoboldCPP PID:' $(pgrep -f koboldcpp)
"""
    
    success, out, err = ssh_run(host, user, launch_cmd, verbose=True)
    if success:
        print(f"  ✓ Launch initiated")
        print(f"    {out}")
    else:
        print(f"  ✗ Launch failed: {err}")
        return False
    
    print()
    return True

def action_test_connection(host, user, port):
    """Test LLM API connectivity"""
    print("╔" + "═"*78 + "╗")
    print("║ STEP 4: TEST LLM CONNECTIVITY                                               ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    print(f"Testing connection to http://{host}:{port}/api/v1/generate...")
    print()
    
    # Wait for server startup
    import time
    time.sleep(5)
    
    # Test with curl
    test_cmd = f"""
curl -s -X POST http://127.0.0.1:{port}/api/v1/generate \\
  -H 'Content-Type: application/json' \\
  -d '{{"prompt":"test","max_tokens":1}}' | head -c 200
"""
    
    success, out, err = ssh_run(host, user, test_cmd, verbose=True)
    if success and ('error' not in out.lower() or len(out) > 50):
        print(f"  ✓ Server responding: {out[:100]}...")
    else:
        print(f"  ✗ Server not responding yet. Check log:")
        print(f"    tail -f ~/koboldcpp.log")
    
    print()

def main():
    parser = argparse.ArgumentParser(description="Deploy KoboldCPP to CESAROPS3 Xeon")
    parser.add_argument('--host', default=XENON_HOST, help="Xeon hostname/IP")
    parser.add_argument('--user', default=XENON_USER, help="SSH user")
    parser.add_argument('--model', default="/models/llama3-8b.gguf", help="GGUF model path")
    parser.add_argument('--port', type=int, default=5001, help="KoboldCPP port")
    parser.add_argument('--action', choices=['diagnose', 'install', 'launch', 'test', 'full'],
                       default='diagnose', help="Action to perform")
    
    args = parser.parse_args()
    
    print()
    print("╔" + "═"*78 + "╗")
    print("║ CESAROPS3 XEON DEPLOYMENT HELPER                                            ║")
    print("║ GPU Diagnostics + KoboldCPP Launch                                          ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    if args.action in ['diagnose', 'full']:
        if not action_diagnose(args.host, args.user):
            print("\n⚠ GPU diagnostics failed. Check connection and NVIDIA setup.")
            if args.action == 'diagnose':
                sys.exit(1)
    
    if args.action in ['install', 'full']:
        action_install_drivers(args.host, args.user)
    
    if args.action in ['launch', 'full']:
        if action_launch_koboldcpp(args.host, args.user, args.model, args.port):
            if args.action == 'launch':
                action_test_connection(args.host, args.user, args.port)
    
    if args.action in ['test']:
        action_test_connection(args.host, args.user, args.port)
    
    print("╔" + "═"*78 + "╗")
    print("║ Deployment Summary                                                          ║")
    print("├" + "─"*78 + "┤")
    print("║                                                                              ║")
    print("║  Next Steps:                                                                 ║")
    print(f"║  1. SSH into Xeon: ssh {args.user}@{args.host}                    ║")
    print("║  2. Monitor koboldcpp: tail -f ~/koboldcpp.log                                ║")
    print(f"║  3. Test API: curl http://{args.host}:{args.port}/api/v1/models       ║")
    print("║  4. Update VSCode agent config to point to this server                      ║")
    print("║                                                                              ║")
    print("╚" + "═"*78 + "╝")
    print()

if __name__ == "__main__":
    main()
