import torch
print("CUDA available:", torch.cuda.is_available())
if torch.cuda.is_available():
    print("GPU:", torch.cuda.get_device_name(0))
    print("VRAM:", torch.cuda.get_device_properties(0).total_memory // 1024**2, "MB")
    # Quick test
    x = torch.ones(100, 100).cuda()
    print("CUDA compute: OK")
else:
    print("No CUDA - check driver/torch version match")
    import subprocess
    r = subprocess.run(["nvcc", "--version"], capture_output=True, text=True)
    print(r.stdout.strip())
