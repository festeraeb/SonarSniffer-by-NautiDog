# KoboldCPP Multi-Machine Deployment - SUCCESS ✅

## Current Status

### ✅ Xeon (100.102.158.111) - OPERATIONAL
- **Service**: KoboldCPP running on port 5001
- **Model**: Qwen2.5-Coder-32B-Instruct-Q3_K_M 
- **GPU**: NVIDIA GeForce GTX 1070 (8GB)
- **GPU Layers**: 11 (out of 65 model layers)
- **API Endpoint**: `http://100.102.158.111:5001/api/v1`
- **Status**: ✅ Running and responsive
- **Model Path**: `/home/cesarops1/ai_coding/models/Qwen2.5-Coder-32B-Instruct-Q3_K_M.gguf`
- **Binary Path**: `/home/cesarops1/ai_coding/koboldcpp`

### 🔄 P1000 (100.105.77.74) - Ready for deployment
- **GPU**: NVIDIA P106-100 (3GB)
- **Status**: Awaiting KoboldCPP installation and model download
- **Planned Role**: Supplementary GPU for distributed inference
- **Recommended Model**: Smaller 7B model for limited VRAM

---

## API Testing

Test from Windows machine:

```powershell
# List available models
Invoke-WebRequest -Uri "http://100.102.158.111:5001/api/v1/models" -UseBasicParsing

# Test completion
$body = @{
    prompt = "def fibonacci("
    max_tokens = 50
} | ConvertTo-Json

Invoke-WebRequest -Uri "http://100.102.158.111:5001/api/v1/completions" `
  -Method POST `
  -Headers @{"Content-Type"="application/json"} `
  -Body $body `
  -UseBasicParsing
```

---

## Deployment Scripts

### 1. **koboldcpp-launcher.sh** (On Xeon/p1000)
```bash
bash /home/cesarops1/koboldcpp-launcher.sh 'Xeon' '/home/cesarops1/ai_coding/models/Qwen2.5-Coder-32B-Instruct-Q3_K_M.gguf' 5001 11
```

### 2. **deploy_kobold_multi.py** (Windows)
```powershell
# Check status of all machines
.venv\Scripts\python.exe deploy_kobold_multi.py --action status

# Deploy to Xeon
.venv\Scripts\python.exe deploy_kobold_multi.py --action deploy --machines xeon

# Stop service
.venv\Scripts\python.exe deploy_kobold_multi.py --action stop --machines xeon

# Restart service
.venv\Scripts\python.exe deploy_kobold_multi.py --action restart --machines xeon
```

### 3. **kobold_dashboard.py** (Web UI - Windows)
```powershell
.venv\Scripts\python.exe kobold_dashboard.py --port 8080
# Then visit: http://localhost:8080
```

---

## Model Information

**Qwen2.5-Coder-32B-Instruct-Q3_K_M**
- **Size**: 32 billion parameters  
- **Quantization**: Q3_K_M (3-bit)
- **Format**: GGUF
- **Specialized in**: Code generation and understanding
- **Context**: 32K tokens max
- **VRAM Usage**: 
  - GPU (11 layers): ~2.8 GB (GTX 1070)
  - CPU (54 layers): ~12 GB (shared system RAM)
  - Total: ~15 GB (available on Xeon with 32GB system RAM)

---

## Next Steps

### Immediate (Optional)
- [ ] Test web dashboard (`kobold_dashboard.py`)
- [ ] Integrate with `cesar_agent_llm.py` for local AI code assistant
- [ ] Configure systemd service for auto-start on reboot

### Short-term
- [ ] Install KoboldCPP on p1000 
- [ ] Download smaller model (7B) for p1000
- [ ] Configure layer distribution across both GPUs
- [ ] Test multi-machine inference pipeline

### Long-term  
- [ ] Auto-scaling based on system load
- [ ] Model swapping for different task types
- [ ] Load balancing between Xeon and p1000
- [ ] Monitoring and logging

---

## Troubleshooting

### Service won't start
```bash
# Check if port is already in use
ssh cesarops1@100.102.158.111 "lsof -i :5001"

# Check logs
ssh cesarops1@100.102.158.111 "tail -100 ~/koboldcpp-5001.log"

# Kill existing process
ssh cesarops1@100.102.158.111 "pkill -f 'koboldcpp.*--port 5001'"
```

### Model loading issues
```bash
# Verify model file exists
ssh cesarops1@100.102.158.111 "ls -lh /home/cesarops1/ai_coding/models/"

# Check VRAM
ssh cesarops1@100.102.158.111 "nvidia-smi"
```

### Network connectivity
```powershell
# Test Tailscale connectivity
ping 100.102.158.111

# Test API connectivity
curl.exe http://100.102.158.111:5001/api/v1/models
```

---

## Configuration Files

- **deploy_kobold_multi.py**: Multi-machine deployment manager
  - Machine definitions: `MACHINES` dict
  - Model paths per machine
  - GPU layer calculations
  
- **kobold_dashboard.py**: Web UI for deployment control
  - REST API endpoints for status and actions
  - Real-time machine monitoring

- **koboldcpp-launcher.sh**: Cross-machine launcher script
  - Auto-detects koboldcpp binary
  - Handles process lifecycle
  - Manages logs and startup verification

---

## Architecture Diagram

```
┌─ Windows (VSCode + Scripts)
│  ├─ deploy_kobold_multi.py
│  ├─ kobold_dashboard.py (web UI)
│  └─ cesarops_agent_llm.py (AI assistant)
│
├─ Tailscale VPN (100.102.158.111 / 100.105.77.74)
│
├─ Xeon Server (100.102.158.111)
│  ├─ KoboldCPP on port 5001
│  ├─ Model: Qwen2.5-Coder-32B
│  ├─ GPU: GTX 1070 (8GB, 11 layers)
│  └─ CPU: 32GB system RAM (54 layers)
│
└─ p1000 Server (100.105.77.74) [Ready for deployment]
   ├─ GPU: P106 (3GB)
   └─ Role: Supplementary GPU (pending)
```

---

## Key Files Generated

1. **koboldcpp-launcher.sh** - Bash launcher for both machines
2. **deploy_kobold_multi.py** - Python CLI deployment manager  
3. **kobold_dashboard.py** - Flask web dashboard
4. **quick_gpu_status.py** - GPU status checker
5. **scripts/diagnose_xeon_gpu.py** - GPU diagnostics
6. **scripts/launch_koboldcpp.py** - Intelligent launcher
7. **CESAROPS3_GPU_FIX.md** - BIOS/driver troubleshooting guide

---

## Integration Points

### Integration with VSCode AI Assistant
Update `cesar_agent_llm.py` to use:
```python
LLM_HOST = "http://100.102.158.111:5001"
# or with load-balancing: http://100.105.77.74:5002 (when p1000 is ready)
```

### Model can now serve:
- Code completion
- Code explanation  
- Bug detection
- Refactoring suggestions
- Documentation generation

All using **100% local inference** without external API calls!

---

## Session Summary

✅ **Objective Achieved**: Successfully deployed KoboldCPP to Xeon with Qwen2.5 Coder 32B model
- Resolved binary path issue
- Verified API accessibility from Windows via Tailscale  
- Created multi-machine deployment infrastructure
- Ready for VSCode AI integration

🎯 **Ready for**: Local AI-powered code assistant without Copilot dependency
