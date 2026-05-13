#!/usr/bin/env python3
"""
MULTI-MACHINE KOBOLDCPP DEPLOYMENT MANAGER
Deploy and manage KoboldCPP across multiple GPU machines

Features:
- Deploy to T440 dual-P100, Xeon, p1000 (i7), or gtx1060
- Auto-detect available GPUs per machine
- Optimized layer distribution
- Web dashboard integration ready
- Stop/restart individual instances
- Health check and monitoring

Machines:
- T440 (cesarops@10.0.0.61): dual Tesla P100 (2x16GB)
- Xeon (cesarops1@100.102.158.111): GTX 1070 (8GB)
- p1000 (cesarops@100.105.77.74): NVIDIA P106 (3GB)
- gtx1060 (cesarops@10.0.0.204): GTX 1060 (6GB)

Usage:
    python3 deploy_kobold_multi.py --action deploy --machines xeon,p1000
    python3 deploy_kobold_multi.py --action status
    python3 deploy_kobold_multi.py --action stop --machine xeon
"""

import subprocess
import json
import argparse
import sys
import time
from dataclasses import dataclass
from typing import Dict, List, Optional
from pathlib import Path

@dataclass
class Machine:
    """Machine configuration"""
    name: str
    host: str
    user: str
    alt_user: str = ""
    password: str = None  # Optional, uses SSH key by default
    port: int = 22
    gpu_type: str = "unknown"
    total_memory_gb: int = 0
    kobold_port: int = 5001
    priority: int = 100
    default_gpu_ids: str = "0"
    
MACHINES = {
    't440_dual_p100': Machine(
        name='t440-dual-p100',
        host='10.0.0.61',
        user='cesarops',
        alt_user='cesarios',
        gpu_type='Tesla P100 x2',
        total_memory_gb=32,
        kobold_port=5001,
        priority=1,
        default_gpu_ids='0,1'
    ),
    'xeon': Machine(
        name='Xeon',
        host='100.102.158.111',
        user='cesarops1',
        gpu_type='GTX 1070',
        total_memory_gb=8,
        kobold_port=5001,
        priority=2,
        default_gpu_ids='0'
    ),
    'p1000': Machine(
        name='p1000',
        host='100.105.77.74',
        user='cesarops',
        gpu_type='NVIDIA P106',
        total_memory_gb=3,
        kobold_port=5002,
        priority=3,
        default_gpu_ids='0'
    ),
    'gtx1060': Machine(
        name='gtx1060',
        host='10.0.0.204',
        user='cesarops',
        gpu_type='GTX 1060',
        total_memory_gb=6,
        kobold_port=5001,
        priority=4,
        default_gpu_ids='0'
    )
}

MODEL_PROFILES = {
    # NOTE: paths are expected to exist on the remote host(s)
    'qwen35-a3b-q8': {
        'model_path': '/mnt/garmour/models/Qwen3.5-35B-A3B-Instruct-Q8_0.gguf',
        'contextsize': 65536,
        'recommended_machine': 't440_dual_p100',
    },
    'qwen25-coder-32b-q4': {
        'model_path': '/mnt/garmour/models/Qwen2.5-Coder-32B-Instruct-Q4_K_M.gguf',
        'contextsize': 65536,
        'recommended_machine': 't440_dual_p100',
    },
    'deepseek-r1-distill-qwen-7b-q8': {
        'model_path': '/mnt/garmour/models/DeepSeek-R1-Distill-Qwen-7B-Q8_0.gguf',
        'contextsize': 65536,
        'recommended_machine': 'xeon',
    },
}


def get_priority_order(only_online: bool = False) -> List[str]:
    ordered = sorted(MACHINES.keys(), key=lambda m: MACHINES[m].priority)
    if not only_online:
        return ordered
    online = []
    for machine_key in ordered:
        machine = MACHINES[machine_key]
        gpu_status = check_gpu_status(machine)
        if gpu_status['available']:
            online.append(machine_key)
    return online

def ssh_run(machine: Machine, cmd: str, timeout=30) -> tuple[bool, str, str]:
    """Execute SSH command on remote machine"""
    users = [machine.user]
    if machine.alt_user:
        users.append(machine.alt_user)

    last_err = ""
    for user in users:
        try:
            full_cmd = f'ssh -o ConnectTimeout=5 {user}@{machine.host} "{cmd}"'
            result = subprocess.run(
                full_cmd,
                shell=True,
                capture_output=True,
                text=True,
                timeout=timeout
            )
            if result.returncode == 0:
                return True, result.stdout.strip(), result.stderr.strip()
            last_err = result.stderr.strip() or result.stdout.strip() or f"ssh failed for {user}@{machine.host}"
        except subprocess.TimeoutExpired:
            last_err = "Timeout"
        except Exception as e:
            last_err = str(e)
    return False, "", last_err

def check_gpu_status(machine: Machine) -> Dict:
    """Check GPU availability on machine"""
    success, out, err = ssh_run(machine, "nvidia-smi -L")
    
    gpu_count = 0
    if success:
        gpu_count = len([l for l in out.split('\n') if l.strip()])
    
    return {
        'machine': machine.name,
        'host': machine.host,
        'gpu_count': gpu_count,
        'gpu_type': machine.gpu_type,
        'memory_gb': machine.total_memory_gb,
        'available': gpu_count > 0,
        'output': out if success else err
    }

def get_kobold_status(machine: Machine) -> Dict:
    """Check if KoboldCPP is running"""
    success, out, err = ssh_run(machine, f"curl -s http://localhost:{machine.kobold_port}/api/v1/models 2>/dev/null | head -c 50")
    
    return {
        'machine': machine.name,
        'port': machine.kobold_port,
        'running': 'error' not in out.lower() and len(out) > 10,
        'response': out[:100] if success else "Offline"
    }

def calculate_gpu_layers(gpu_memory_gb: int, model_size_gb: float = 4.5) -> int:
    """Calculate optimal GPU layers"""
    # Conservative: reserve 1.5GB for OS
    available = max(gpu_memory_gb - 1.5, 0.5)
    
    # For Q4 models: ~0.5-0.7GB per layer
    layers_per_gb = 1.8
    layers = int(available * layers_per_gb)
    
    # Cap at reasonable max
    return min(layers, 32)

def generate_kobold_cmd(
    machine: Machine,
    model_path: str,
    gpu_mode: str = "both",
    contextsize: int = 65536,
) -> str:
    """Generate KoboldCPP launch command using smart launcher script."""
    if gpu_mode not in {"one", "both"}:
        gpu_mode = "both"

    gpu_ids = machine.default_gpu_ids
    if gpu_mode == "one":
        gpu_ids = machine.default_gpu_ids.split(',')[0]

    cmd = (
        f"python3 ~/launch_koboldcpp.py "
        f"--model '{model_path}' "
        f"--port {machine.kobold_port} "
        f"--gpu-ids {gpu_ids} "
        f"--contextsize {contextsize}"
    )
    return cmd

def deploy_kobold(
    machine: Machine,
    model_path: str = "/models/llama3-8b.gguf",
    gpu_mode: str = "both",
    contextsize: int = 65536,
) -> bool:
    """Deploy KoboldCPP to a machine"""
    print(f"\n📦 Deploying to {machine.name} ({machine.host})...")
    print(f"   GPU: {machine.gpu_type} ({machine.total_memory_gb}GB)")
    
    # Check GPU first
    gpu_status = check_gpu_status(machine)
    if not gpu_status['available']:
        print(f"   ❌ No GPU detected!")
        return False
    
    print(f"   ✓ GPU available: {gpu_status['gpu_count']} device(s)")
    
    # Generate and run launch command
    cmd = generate_kobold_cmd(machine, model_path, gpu_mode=gpu_mode, contextsize=contextsize)
    success, out, err = ssh_run(machine, cmd, timeout=20)
    
    if success:
        print(f"   ✓ Deployment command sent")
        print(f"   📊 Output: {out}")
        # Wait a moment for startup
        time.sleep(3)
        
        # Verify it's running
        status = get_kobold_status(machine)
        if status['running']:
            print(f"   ✅ KoboldCPP running on port {machine.kobold_port}")
            return True
        else:
            print(f"   ⏳ Starting (check logs in ~10s)")
            return True
    else:
        print(f"   ❌ Deployment failed: {err}")
        return False

def stop_kobold(machine: Machine) -> bool:
    """Stop KoboldCPP on a machine"""
    print(f"\n🛑 Stopping {machine.name}...")
    success, out, err = ssh_run(machine, "pkill -f koboldcpp && echo 'Stopped'")
    
    if success:
        print(f"   ✓ KoboldCPP stopped")
        return True
    else:
        print(f"   ⚠ Already stopped or error: {err}")
        return True

def status_report(machines_list: List[str]) -> None:
    """Generate status report for machines"""
    print("\n╔" + "═"*70 + "╗")
    print("║ MULTI-MACHINE KOBOLDCPP STATUS REPORT                              ║")
    print("╚" + "═"*70 + "╝")
    print()
    
    for machine_key in machines_list:
        if machine_key not in MACHINES:
            continue
        machine = MACHINES[machine_key]
        
        print(f"┌─ {machine.name.upper()} ({machine.host})")
        print("│")
        
        # GPU Status
        gpu_status = check_gpu_status(machine)
        gpu_icon = "✓" if gpu_status['available'] else "✗"
        print(f"│  GPU:    {gpu_icon} {gpu_status['gpu_type']} x{gpu_status['gpu_count']}")
        print(f"│  Memory: {gpu_status['memory_gb']}GB total")
        print(f"│  Priority: {machine.priority}")
        print(f"│  Layers: ~{calculate_gpu_layers(gpu_status['memory_gb'])} layers to GPU")
        
        # KoboldCPP Status
        kobold_status = get_kobold_status(machine)
        kobold_icon = "✓" if kobold_status['running'] else "✗"
        status_text = "Running" if kobold_status['running'] else "Offline"
        print(f"│  Server: {kobold_icon} {status_text} (port {kobold_status['port']})")
        
        print("│")

def main():
    parser = argparse.ArgumentParser(description="Multi-Machine KoboldCPP Deployment Manager")
    parser.add_argument('--action', choices=['deploy', 'status', 'stop', 'restart'],
                       default='status', help="Action to perform")
    parser.add_argument('--machines', help="Comma-separated machine list (xeon,p1000)")
    parser.add_argument('--model', default=None, help="GGUF model path (uses machine defaults if not specified)")
    parser.add_argument('--profile', default=None, choices=list(MODEL_PROFILES.keys()), help="Named model profile")
    parser.add_argument('--gpu-mode', default='both', choices=['one', 'both'], help="Use one GPU or both GPUs when available")
    parser.add_argument('--contextsize', type=int, default=None, help="Context size override")
    parser.add_argument('--machine', help=f"Single machine ({', '.join(MACHINES.keys())})")
    
    args = parser.parse_args()
    
    # Model paths per machine
    MODEL_PATHS = {
        't440_dual_p100': '/mnt/garmour/models/Qwen2.5-Coder-32B-Instruct-Q4_K_M.gguf',
        'xeon': '/home/cesarops1/ai_coding/models/Qwen2.5-Coder-32B-Instruct-Q3_K_M.gguf',
        'p1000': '/mnt/garmour/models/qwen-7b.gguf',
        'gtx1060': '/mnt/garmour/models/DeepSeek-R1-Distill-Qwen-7B-Q8_0.gguf'
    }
    
    # Determine target machines
    if args.machine:
        target_machines = [args.machine]
    elif args.machines:
        target_machines = args.machines.split(',')
    else:
        # Default to online machines in explicit priority order
        target_machines = get_priority_order(only_online=True) or get_priority_order(only_online=False)
    
    # Validate machines
    for m in target_machines:
        if m not in MACHINES:
            print(f"Unknown machine: {m}")
            print(f"Available: {', '.join(MACHINES.keys())}")
            sys.exit(1)
    
    print("\n╔" + "═"*70 + "╗")
    print("║ MULTI-MACHINE KOBOLDCPP DEPLOYMENT MANAGER                          ║")
    print("╚" + "═"*70 + "╝")
    
    if args.action == 'status':
        status_report(target_machines)
        print("\nModel profiles:")
        for profile, p in MODEL_PROFILES.items():
            print(f"  - {profile}: {p['model_path']} (ctx={p['contextsize']}, recommended={p['recommended_machine']})")
    
    elif args.action == 'deploy':
        print(f"\n🚀 Deploying to: {', '.join(target_machines)}")
        profile = MODEL_PROFILES.get(args.profile) if args.profile else None
        
        for machine_key in target_machines:
            # Use specified model or machine default
            model_path = args.model or (profile['model_path'] if profile else MODEL_PATHS.get(machine_key, '/models/llama3-8b.gguf'))
            contextsize = args.contextsize or (profile['contextsize'] if profile else 65536)
            print(f"📦 Model: {model_path}")
            print(f"🎛  GPU mode: {args.gpu_mode}, context: {contextsize}")
            print()
            
            machine = MACHINES[machine_key]
            deploy_kobold(machine, model_path, gpu_mode=args.gpu_mode, contextsize=contextsize)
        
        print("\n✅ Deployment complete!")
        time.sleep(2)
        status_report(target_machines)
    
    elif args.action == 'stop':
        print(f"\n🛑 Stopping: {', '.join(target_machines)}")
        for machine_key in target_machines:
            machine = MACHINES[machine_key]
            stop_kobold(machine)
        print("\n✓ All stopped")
    
    elif args.action == 'restart':
        print(f"\n🔄 Restarting: {', '.join(target_machines)}")
        profile = MODEL_PROFILES.get(args.profile) if args.profile else None
        for machine_key in target_machines:
            machine = MACHINES[machine_key]
            stop_kobold(machine)
        time.sleep(2)
        for machine_key in target_machines:
            # Use specified model or machine default
            model_path = args.model or (profile['model_path'] if profile else MODEL_PATHS.get(machine_key, '/models/llama3-8b.gguf'))
            contextsize = args.contextsize or (profile['contextsize'] if profile else 65536)
            machine = MACHINES[machine_key]
            deploy_kobold(machine, model_path, gpu_mode=args.gpu_mode, contextsize=contextsize)
        print("\n✅ Restart complete!")
        status_report(target_machines)

if __name__ == "__main__":
    main()
