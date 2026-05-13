#!/bin/bash
# Setup Coral TPU via userspace libedgetpu (no kernel gasket module needed)
set -e

echo "=== Coral TPU Userspace Setup ==="

# Add Coral repo if not present
if [ ! -f /etc/apt/sources.list.d/coral-edgetpu.list ]; then
    echo "Adding Coral apt repository..."
    echo "deb https://packages.cloud.google.com/apt coral-edgetpu-stable main" | sudo tee /etc/apt/sources.list.d/coral-edgetpu.list
    curl -fsSL https://packages.cloud.google.com/apt/doc/apt-key.gpg | sudo apt-key add -
    sudo apt-get update -qq
fi

# Install libedgetpu (userspace USB/PCIe access)
echo "Installing libedgetpu1-std..."
sudo apt-get install -y libedgetpu1-std 2>&1 | tail -3

# Install Python bindings
echo "Installing pycoral + tflite-runtime..."
pip3 install --break-system-packages pycoral tflite-runtime 2>&1 | tail -3

# Check PCIe device
echo ""
echo "PCIe device:"
lspci | grep -i coral

# Try to access via libedgetpu
echo ""
echo "Testing TPU access..."
python3 -c "
from pycoral.utils.edgetpu import list_edge_tpus
tpus = list_edge_tpus()
if tpus:
    print(f'Found {len(tpus)} Edge TPU(s):')
    for t in tpus:
        print(f'  {t}')
else:
    print('No Edge TPUs found via libedgetpu')
    print('Note: PCIe TPU without gasket driver needs /dev/apex_0')
    print('Checking if apex device exists...')
    import os
    if os.path.exists('/dev/apex_0'):
        print('  /dev/apex_0 EXISTS')
    else:
        print('  /dev/apex_0 NOT FOUND - gasket driver needed for PCIe TPU')
        print('  Options:')
        print('    1. Build gasket from source for kernel $(uname -r)')
        print('    2. Use USB Coral accelerator instead')
        print('    3. Boot older kernel with gasket-dkms support')
" 2>&1

echo ""
echo "=== TPU Setup Complete ==="
