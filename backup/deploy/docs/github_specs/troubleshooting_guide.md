# CESAROPS Installation Troubleshooting Guide

## 🚨 Installation Stalled During "Enhanced Plotting"?

**This is a common issue with matplotlib/plotly installation. Here's how to fix it:**

### Quick Fix (Recommended)
1. **Cancel the current installation** (Ctrl+C if still running)
2. **Use the quick install script**: `cesarops_quick_install.bat`  
3. **Skip plotting libraries** - they're not required for core SAR functionality

### Manual Recovery Steps

```batch
# 1. Delete the current environment
rmdir /s /q venv

# 2. Create fresh environment  
python -m venv venv
call venv\Scripts\activate.bat

# 3. Install ONLY essential packages
pip install pandas numpy requests PyYAML scipy

# 4. Test core functionality
python -c "import pandas, numpy, requests, yaml, scipy; print('Core OK')"
```

## 🔧 Common Installation Problems

### Problem: "pip install failed" 
**Solutions:**
- Add `--no-cache-dir` flag: `pip install --no-cache-dir pandas`
- Try individual packages: `pip install pandas` then `pip install numpy` etc.
- Update pip first: `python -m pip install --upgrade pip`

### Problem: "tkinter not available"
**Solutions:**
- **Windows**: Reinstall Python from python.org (make sure "tcl/tk" is checked)
- **Alternative**: The app will still work, just no GUI (command line mode)

### Problem: "Microsoft Visual C++ required"
**Solutions:**
- Download "Microsoft C++ Build Tools" from Microsoft
- Or install "Visual Studio Community" (free)
- Alternative: Use pre-compiled wheels: `pip install --only-binary=all scipy`

### Problem: "SSL Certificate error"
**Solutions:**
- Use trusted hosts: `pip install --trusted-host pypi.org --trusted-host pypi.python.org pandas`
- Update certificates: `pip install --upgrade certifi`

### Problem: "Permission denied"
**Solutions:**
- Run as administrator
- Or use user install: `pip install --user pandas`

## 📦 What You Actually Need

### Essential (Must Have):
- ✅ pandas - Data processing  
- ✅ numpy - Numerical computing
- ✅ requests - Download ocean data
- ✅ PyYAML - Configuration files
- ✅ scipy - Scientific computing
- ✅ tkinter - GUI (usually included with Python)

### Optional (Nice to Have):
- 🔶 simplekml - Export KML files for Google Earth
- 🔶 scikit-learn + joblib - Machine learning enhancements

### Not Essential (Skip if Problems):
- ❌ matplotlib - Plotting (causes most install issues)
- ❌ plotly - Interactive plots (not used in core features)
- ❌ jupyter - Notebooks (not needed)

## 🏃‍♂️ Quick Start Without Full Install

If you're having persistent issues, you can run CESAROPS with minimal dependencies:

1. **Install Python 3.8+**
2. **Install only core packages:**
   ```
   pip install pandas numpy requests PyYAML scipy
   ```
3. **Download main script:** `cesarops_enhanced.py`
4. **Create simple config:** `config.yaml`
5. **Run:** `python cesarops_enhanced.py`

## 🌐 Alternative Installation Methods

### Method 1: Anaconda (Recommended for Windows)
```batch
# Download Anaconda from anaconda.com
conda create -n cesarops python=3.10
conda activate cesarops
conda install pandas numpy requests pyyaml scipy
pip install simplekml  # Only if needed
```

### Method 2: Pre-built Environment
```batch
# Use Python from Microsoft Store (Windows 10/11)
# It includes tkinter and has fewer permission issues
python -m pip install pandas numpy requests PyYAML scipy
```

### Method 3: Docker (Advanced Users)
```dockerfile
FROM python:3.10-slim
WORKDIR /app
COPY requirements_minimal.txt .
RUN pip install -r requirements_minimal.txt
COPY . .
CMD ["python", "cesarops_enhanced.py"]
```

## 🔍 Testing Your Installation

Run this test to verify everything works:

```python
# test_installation.py
try:
    import pandas as pd
    import numpy as np
    import requests
    import yaml
    import scipy
    print("✓ All core packages working!")
    
    # Test GUI
    import tkinter as tk
    root = tk.Tk()
    root.destroy()
    print("✓ GUI support working!")
    
    # Test optional packages
    try:
        import sklearn
        print("✓ ML support available")
    except ImportError:
        print("- ML support not installed (optional)")
    
    try:
        import simplekml
        print("✓ KML export available")
    except ImportError:
        print("- KML export not installed (optional)")
        
except ImportError as e:
    print(f"✗ Missing required package: {e}")
    print("Run: pip install pandas numpy requests PyYAML scipy")
```

## 🆘 Still Having Problems?

### Check System Requirements:
- **Python 3.8+** (not 3.7 or older)
- **Windows 7+** / **macOS 10.14+** / **Linux with glibc 2.17+**
- **4 GB RAM** minimum (8 GB recommended)
- **1 GB disk space** for Python packages
- **Internet connection** for initial download

### Get Help:
1. **Check the error message** - often tells you exactly what's missing
2. **Try minimal install first** - get basic functionality working
3. **Add features gradually** - install optional components one by one
4. **Document your system** - Python version, OS version, error messages

### Emergency Workaround:
If nothing else works, CESAROPS can run in "offline mode" with pre-downloaded data files. Contact support for emergency SAR situations.

## 📝 Reporting Installation Issues

If you need help, please include:

```
System: Windows 10/11, macOS, Ubuntu, etc.
Python: python --version
Pip: pip --version  
Error: [paste full error message]
Command: [the exact command that failed]
```

**Remember: The goal is saving lives, not perfect installations. Get the core working first, add features later!**