# CESAROPS3 (XEON) GPU FIX - Comprehensive Guide

**Status**: GPU visibility reduced from 2 → 1 after BIOS 4G BAR update  
**Goal**: Restore dual-GPU recognition and get koboldcpp running  
**Hardware**: Xeon with GTX 1070 + GTX 1060 (only one visible post-BIOS)

---

## 🔍 Problem Analysis

### What Happened
You performed a BIOS update to address the "4G BAR issue" (enable above-4GB PCIe address space). This is typically needed for systems with multiple GPUs and >4GB of VRAM per card. However, the update appears to have inadvertently:

1. **Disabled PCIe slot discovery** for the second GPU
2. **Changed IOMMU/PCIe enumeration** settings
3. **Not properly POST'd** the second GPU during boot

### Root Causes (Most Likely)
- IOMMU disabled or incorrectly configured
- Second PCIe slot set to "off" in BIOS
- GPU not powered (check power connectors on both cards)
- Driver version incompatibility with new BIOS settings
- Kernel parameter changes needed for dual-GPU support

---

## 🛠️ Solution Steps

### Step 1: SSH Access Verification
First, ensure you can connect to cesarops3:
```bash
ssh cesarops@10.0.0.56
# Should prompt for password or use key
```

### Step 2: Run GPU Diagnostic
Execute the diagnostic script from your local machine:
```bash
python3 scripts/diagnose_xeon_gpu.py
# or with custom host:
python3 scripts/diagnose_xeon_gpu.py 10.0.0.56 cesarops
```

**Expected Output (if issue exists):**
- `✓ Driver installed` but only 1 GPU listed
- `CUDA Devices: 1` (should be 2)
- `lspci | grep nvidia` shows only 1 card

### Step 3: BIOS Fixes (On Xeon Console)

**Login to BIOS Setup** (usually Delete or F2 during boot):

1. **Enable Second PCIe Slot:**
   - Navigate to: `Integrated Peripherals` → `PCIe Configuration` (varies by BIOS)
   - Ensure all PCIe slots are enabled (Slot 1, Slot 2, etc.)
   - Verify "PCI 64-Bit Resources" is enabled
   - Set "PCIe CLKREQ" to Enabled

2. **Configure IOMMU (for GPU passthrough):**
   - `Advanced` → `IOMMU`
   - Set to: `Enabled` or `Auto`
   - Save and exit BIOS

3. **Verify 4G BAR Setting:**
   - `Integrated Peripherals` → `4G Decoding` → `Enabled` (this was your original fix)
   - Should remain enabled

4. **Reboot and verify with this command on Xeon:**
   ```bash
   lspci | grep -i nvidia
   # Should show TWO entries
   
   nvidia-smi -L
   # Should list GPU 0 and GPU 1
   ```

### Step 4: Verify GPU Power & Connections

**Physical Check** (Server room access needed):
- Ensure BOTH GPUs have 6-pin or 8-pin PCIe power connectors plugged in
- Try reseating cards if they came loose
- Check that second GPU has proper cooling airflow

**Linux Check:**
```bash
# SSH into Xeon
ssh cesarops@10.0.0.56

# Check PCIe power state
sudo dmesg | grep -i "suspend\|power" | tail -5

# Check if second GPU appears in ACPI
sudo ls -la /proc/acpi/find_device | grep PCI
```

### Step 5: Update NVIDIA Drivers

**On Xeon Console** (or via SSH with `sudo`):
```bash
ssh cesarops@10.0.0.56

# Check current driver version
nvidia-smi --query-gpu=driver_version --format=csv,noheader

# Update drivers (run this AFTER BIOS changes)
sudo apt-get update
sudo apt-get install -y nvidia-driver-555
# (or latest version available: apt-cache search nvidia-driver | grep "^nvidia-driver" | tail -1)

# Reboot to apply
sudo reboot

# Wait ~60s then verify
nvidia-smi -L
# Should show both GPUs now!
```

### Step 6: Verify CuPy/CUDA Recognition

```bash
ssh cesarops@10.0.0.56

python3 -c "
import cupy as cp
count = cp.cuda.runtime.getDeviceCount()
print(f'CUDA sees {count} GPU(s)')
for i in range(count):
    dev = cp.cuda.Device(i)
    props = cp.cuda.runtime.getDeviceProperties(i)
    print(f'  GPU {i}: {props[\"name\"].decode()} - {props[\"totalGlobalMem\"]//1e9:.1f}GB')
"

# Output should show both GPUs with memory
```

---

## 🚀 Step 7: Launch KoboldCPP

### Copy Launcher Scripts to Xeon
```bash
# From your local machine
scp scripts/launch_koboldcpp.py cesarops@10.0.0.56:~/
scp scripts/deploy_cesarops3.py cesarops@10.0.0.56:~/
```

### Option A: Guided Deployment (Local Machine)
```bash
python3 scripts/deploy_cesarops3.py \
  --host 10.0.0.56 \
  --user cesarops \
  --model /models/llama3-8b.gguf \
  --action full
```

### Option B: Manual Launch (Xeon)
```bash
ssh cesarops@10.0.0.56

# Launch in background
nohup python3 ~/launch_koboldcpp.py \
  --model /models/llama3-8b.gguf \
  --port 5001 \
  --verbose \
  > ~/koboldcpp.log 2>&1 &

# Monitor startup
tail -f ~/koboldcpp.log

# Should see output like:
# ✓ Found 2 GPU(s):
#   └─ GPU 0: NVIDIA GeForce GTX 1070 (8GB VRAM, CC 6.1)
#   └─ GPU 1: NVIDIA GeForce GTX 1060 (3GB VRAM, CC 6.1)
# Generated Command:
#   /opt/koboldcpp/koboldcpp-linux-x64 --model /models/llama3-8b.gguf --port 5001 ...
```

### Option C: N8N Workflow Update
Update your n8n "Boot KoboldCPP" node:
```
Old:  /opt/koboldcpp/koboldcpp-linux-x64 --model /models/llama3-8b.gguf --port 5001
New:  python3 ~/launch_koboldcpp.py --model /models/llama3-8b.gguf --port 5001
```

---

## ✅ Verification

### Test 1: Server Running
```bash
curl -s http://10.0.0.56:5001/api/v1/models | jq .
# Should return model info
```

### Test 2: API Test Prompt
```bash
curl -X POST http://10.0.0.56:5001/api/v1/generate \
  -H "Content-Type: application/json" \
  -d '{
    "prompt": "What is machine learning?",
    "max_tokens": 50,
    "temperature": 0.7
  }' | jq .
```

### Test 3: VSCode Agent Connection
Update [cesar_agent_llm.py](../cesar_agent_llm.py):
```python
self.base_url = os.getenv("LLM_HOST", "http://10.0.0.56:5001/v1")
# Was: http://10.0.0.161:5001/v1
```

---

## 📊 Understanding Your GPU Configuration

### What the Launcher Does

The `launch_koboldcpp.py` script automatically:

1. **Detects available GPUs** using CuPy
2. **Calculates optimal layer offloading:**
   - GTX 1070 (8GB) → ~10-12 layers
   - GTX 1060 (3GB) → ~3-4 layers
3. **Generates optimized flags:**
   ```
   --gpulayers 15    # Total layers across both GPUs
   --gpumultiplier 1.5  # Conservative memory usage
   ```
4. **Handles edge cases:**
   - Falls back to CPU if GPU offline
   - Reserves 1.5GB for OS operations
   - Caps layers at model maximum

### Model Sizes (Reference)

| Model | Size | Q4 VRAM | Q5 VRAM | Layers |
|-------|------|---------|---------|--------|
| Llama3 8B | 8B | 4.5GB | 6GB | 32 |
| Qwen-7B | 7B | 3.8GB | 5GB | 28 |
| DeepSeek Coder 6.7B | 6.7B | 3.5GB | 4.5GB | 26 |
| Mistral 7B | 7B | 4.0GB | 5.5GB | 32 |

Your setup (1070 + 1060 = 11GB):
- **Full 8B Q4 models** ✅ Fits with both GPUs
- **Faster inference** with layer distribution across cards

---

## 🔧 Troubleshooting

### GPU Still Not Showing After BIOS Changes

```bash
# Check if GPU is detected at PCIe level
ssh cesarops@10.0.0.56
sudo lspci -vv | grep -A20 "VGA compatible"

# If no PCIe entries:
#   1. Physical reseat of GPU
#   2. Try different PCIe slot
#   3. Contact hardware support

# If PCIe shows but nvidia-smi doesn't:
#   1. Reinstall drivers: sudo apt-get remove nvidia* && sudo apt-get install nvidia-driver-555
#   2. Check kernel module: lsmod | grep nvidia
#   3. Reboot if module not loaded
```

### KoboldCPP Won't Start

```bash
# Check error log
ssh cesarops@10.0.0.56
tail -50 ~/koboldcpp.log

# Common issues:
# - Model file not found → Check path
# - Port already in use → Kill: sudo lsof -i :5001 && kill -9 <PID>
# - GPU memory full → Reduce --gpulayers
# - CUDA error → Reinstall drivers
```

### Low Performance on Second GPU

If GTX 1060 isn't helping (or slowing things down):
- It has only 3GB VRAM vs 1070's 8GB
- May be slower architecture (GP106 vs GP104)
- Option: Use only 1070 for inference: `--gpu 0`

```bash
python3 launch_koboldcpp.py --gpu-ids 0  # Use only GTX 1070
```

---

## 📝 Your Python Modifications (What You Did)

You mentioned: *"i redid some of this in python however I did a bios update"*

The current setup uses:
- `bootstrap_xenon.sh` - Initial GPU detection  
- `cesar_agent_llm.py` - LLM API client (expects KoboldCPP at port 5001)
- `gpu_server.py` - CuPy inference server (uses device 0 hardcoded)

**New scripts replace the manual GPU configuration:**
- `launch_koboldcpp.py` - Smart launcher (multi-GPU aware)
- `diagnose_xeon_gpu.py` - Full diagnostic suite
- `deploy_cesarops3.py` - End-to-end deployment

---

## 📚 References

**NVIDIA Documentation:**
- [Multi-GPU Setup](https://docs.nvidia.com/cuda/cuda-runtime-api/group__CUDART__DEVICE.html)
- [VRAM Allocation](https://docs.nvidia.com/cuda/cuda-c-programming-guide/index.html#device-memory)

**KoboldCPP Flags:**
- `--gpulayers N` - Number of transformer layers to offload to GPU
- `--gpu ID` - GPU device ID (0, 1, etc.)
- `--gpumultiplier X` - VRAM multiplier (1.0 = exact, 1.5 = conservative)

**4G BAR Issue:**
- Common on motherboards with >4GB total GPU VRAM
- Allows PCIe to use 64-bit address space
- Required for proper dual/multi-GPU configuration

---

## ⏭️ Next Steps

1. **Immediate**: Run `python3 scripts/diagnose_xeon_gpu.py`
2. **If 2 GPUs don't show**: Apply BIOS fixes (Step 3)
3. **After BIOS**: Run `python3 scripts/deploy_cesarops3.py --action diagnose` again
4. **Verify both GPUs**: `nvidia-smi -L` should list 2 cards
5. **Launch KoboldCPP**: `python3 scripts/deploy_cesarops3.py --action launch`
6. **Test API**: `curl http://10.0.0.56:5001/api/v1/models`
7. **Update VSCode agent**: Point to `http://10.0.0.56:5001/v1` for LLM queries

---

**Created**: April 24, 2026  
**For**: CESAROPS3 Xeon Multi-GPU Configuration  
**Status**: Ready for deployment

Questions? Check the script comments or run with `--verbose` flag.
