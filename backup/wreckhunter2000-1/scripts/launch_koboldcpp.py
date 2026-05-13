#!/usr/bin/env python3
"""
SMART KOBOLDCPP LAUNCHER for CESAROPS
Automatically detects available GPUs and launches with optimal settings

Features:
- Detects available CUDA/GPU devices
- Allocates layers based on available VRAM
- Supports single and multi-GPU configurations
- Falls back to CPU if no GPU detected
- Generates appropriate CLI flags

Usage:
    python3 launch_koboldcpp.py [--model PATH] [--port PORT] [--gpu-ids 0,1] [--dry-run]
"""

import subprocess
import sys
import argparse
import json
import os
from pathlib import Path

# Defaults
DEFAULT_MODEL = "/models/llama3-8b.gguf"
DEFAULT_PORT = 5001
KOBOLDCPP_PATH = "/opt/koboldcpp/koboldcpp-linux-x64"

def get_gpu_info():
    """Detect available GPUs and memory"""
    gpus = []
    try:
        import cupy as cp
        count = cp.cuda.runtime.getDeviceCount()
        
        for i in range(count):
            try:
                dev = cp.cuda.Device(i)
                props = cp.cuda.runtime.getDeviceProperties(i)
                name = props.get('name', b'Unknown')
                if isinstance(name, bytes):
                    name = name.decode()
                
                mem_total = props.get('totalGlobalMem', 0) // (1024**3)  # GB
                compute = f"{props.get('major', 0)}.{props.get('minor', 0)}"
                
                gpus.append({
                    'index': i,
                    'name': name,
                    'memory_gb': mem_total,
                    'compute_capability': compute,
                    'available': True
                })
            except Exception as e:
                print(f"⚠ GPU {i} error: {e}", file=sys.stderr)
    
    except ImportError:
        print("⚠ CuPy not installed - GPU detection unavailable", file=sys.stderr)
    except Exception as e:
        print(f"⚠ GPU detection error: {e}", file=sys.stderr)
    
    return gpus

def calculate_gpulayers(model_path, gpu_memory_gb, reserve_gb=1.5):
    """
    Estimate optimal number of layers to offload to GPU
    
    Model sizes (approximate):
    - Llama3 8B FP16: ~16GB (32 layers)
    - Llama3 8B Q4: ~4-5GB (32 layers) 
    - Qwen-7B Q4: ~3.5GB (28 layers)
    - DeepSeek Coder 6.7B Q4: ~3.5GB (26 layers)
    
    Layer calculation: (available_vram - reserve) / vram_per_layer
    For 8B models: ~0.5-0.7GB per layer in Q4
    """
    available_gb = gpu_memory_gb - reserve_gb
    
    if available_gb < 2:
        return 0  # Not enough VRAM
    
    # Conservative estimate: 0.6GB per layer for Q4 8B models
    layers_per_gb = 1.5  # layers/GB (for Q4)
    estimated_layers = int(available_gb * layers_per_gb)
    
    # Cap based on model (8B = 32 layers max, 7B = 28 layers max)
    max_layers = 32  # Adjust if using smaller models
    
    return min(estimated_layers, max_layers)

def generate_koboldcpp_cmd(model_path, port, gpu_ids=None, dry_run=False):
    """Generate optimal koboldcpp command"""
    
    gpus = get_gpu_info()
    
    print("╔" + "═"*78 + "╗")
    print("║ KOBOLDCPP LAUNCHER - CESAROPS3                                              ║")
    print("╚" + "═"*78 + "╝")
    print()
    
    # GPU Summary
    if gpus:
        print(f"✓ Found {len(gpus)} GPU(s):")
        for gpu in gpus:
            print(f"  └─ GPU {gpu['index']}: {gpu['name']} ({gpu['memory_gb']}GB VRAM, CC {gpu['compute_capability']})")
        print()
    else:
        print("⚠ No GPUs detected - will use CPU mode")
        print()
    
    # Filter to requested GPUs if specified
    if gpu_ids is not None:
        requested = set(gpu_ids.split(','))
        gpus = [g for g in gpus if str(g['index']) in requested]
        if not gpus:
            print(f"✗ Error: None of requested GPU IDs {requested} found")
            sys.exit(1)
    
    # Build command
    cmd = [KOBOLDCPP_PATH]
    
    # Model
    if not Path(model_path).exists():
        print(f"⚠ Warning: Model path not found: {model_path}")
    cmd.extend(['--model', model_path])
    
    # Port
    cmd.extend(['--port', str(port)])
    
    # GPU Configuration
    if gpus:
        if len(gpus) == 1:
            # Single GPU: use all available layers
            gpu = gpus[0]
            num_layers = calculate_gpulayers(model_path, gpu['memory_gb'])
            print(f"┌─ GPU Configuration (Single GPU)")
            print(f"│  GPU 0: {gpu['name']}")
            print(f"│  Available VRAM: {gpu['memory_gb']}GB")
            print(f"│  Estimated layers for GPU: {num_layers}")
            print(f"│  GPU ID: {gpu['index']}")
            print()
            
            # Add GPU flags
            cmd.extend(['--gpulayers', str(num_layers)])
            cmd.extend(['--gpu', str(gpu['index'])])
            # Memory multiplier: conservative for single GPU
            cmd.extend(['--gpumultiplier', '1.5'])
            
        else:
            # Multi-GPU: distribute layers
            print(f"┌─ GPU Configuration (Multi-GPU)")
            total_vram = sum(g['memory_gb'] for g in gpus)
            print(f"│  Total VRAM: {total_vram}GB")
            
            layer_assignments = {}
            for gpu in gpus:
                layers = calculate_gpulayers(model_path, gpu['memory_gb'])
                layer_assignments[gpu['index']] = layers
                print(f"│  GPU {gpu['index']} ({gpu['name']}): {layers} layers")
            
            print()
            
            # For multi-GPU, we'll specify total layers (koboldcpp distributes)
            total_layers = sum(layer_assignments.values())
            cmd.extend(['--gpulayers', str(total_layers)])
            # Use primary GPU (lowest index)
            cmd.extend(['--gpu', str(gpus[0]['index'])])
            cmd.extend(['--gpumultiplier', '1.0'])
            
    else:
        print("│  Using CPU mode (no GPU optimization)")
        print()
        cmd.append('--cpu')
    
    # Additional optimization flags
    cmd.extend([
        '--contextsize', '4096',      # Reasonable context
        '--threads', '8',              # CPU threads
    ])
    
    print("─" * 80)
    print(f"Generated Command:")
    print()
    cmd_str = ' '.join(cmd)
    print(f"  {cmd_str}")
    print()
    
    return cmd, cmd_str

def main():
    parser = argparse.ArgumentParser(description="Smart Koboldcpp Launcher for CESAROPS")
    parser.add_argument('--model', default=DEFAULT_MODEL, help=f"Model path (default: {DEFAULT_MODEL})")
    parser.add_argument('--port', type=int, default=DEFAULT_PORT, help=f"Port (default: {DEFAULT_PORT})")
    parser.add_argument('--gpu-ids', help="Comma-separated GPU IDs to use (e.g., '0,1')")
    parser.add_argument('--dry-run', action='store_true', help="Show command without running")
    parser.add_argument('--verbose', action='store_true', help="Verbose output")
    
    args = parser.parse_args()
    
    cmd, cmd_str = generate_koboldcpp_cmd(args.model, args.port, args.gpu_ids, args.dry_run)
    
    if args.dry_run:
        print("─" * 80)
        print("[DRY RUN - Command would be executed as shown above]")
        print()
        sys.exit(0)
    
    print("─" * 80)
    print("Starting KoboldCPP...")
    print()
    
    try:
        subprocess.run(cmd)
    except KeyboardInterrupt:
        print("\n\nKoboldCPP terminated by user")
        sys.exit(0)
    except Exception as e:
        print(f"Error launching KoboldCPP: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
