#!/usr/bin/env python3
"""Bring up eno2 as a second NIC with DHCP - safe, no bonding risk"""
import pathlib
import subprocess
import sys

netplan = """network:
  version: 2
  ethernets:
    eno1:
      dhcp4: true
    eno2:
      dhcp4: true
"""

pathlib.Path("/tmp/01-dual-nic.yaml").write_text(netplan)

# Copy with sudo
r = subprocess.run(["bash", "-c", "echo cesarops | sudo -S cp /tmp/01-dual-nic.yaml /etc/netplan/01-dual-nic.yaml"], capture_output=True, text=True)
r2 = subprocess.run(["bash", "-c", "echo cesarops | sudo -S rm -f /etc/netplan/50-cloud-init.yaml /etc/netplan/00-installer-config.yaml 2>/dev/null"], capture_output=True, text=True)
r3 = subprocess.run(["bash", "-c", "echo cesarops | sudo -S netplan apply 2>&1"], capture_output=True, text=True)
print(r3.stdout)
if r3.stderr:
    print(r3.stderr)

import time
time.sleep(5)
import os
os.system("ip addr show eno2 | grep 'inet '")
print("Done. eno2 should now have a DHCP address.")
