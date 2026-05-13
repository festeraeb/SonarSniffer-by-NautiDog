#!/usr/bin/env python3
"""Set up bonded NICs on T440 for redundancy + speed"""
import pathlib
import subprocess

# Write the netplan config
netplan = """network:
  version: 2
  bonds:
    bond0:
      interfaces: [eno1, eno2]
      parameters:
        mode: balance-rr
        mii-monitor-interval: 100
      dhcp4: true
"""

pathlib.Path("/tmp/01-bond.yaml").write_text(netplan)
subprocess.run(["sudo", "-S", "cp", "/tmp/01-bond.yaml", "/etc/netplan/01-bond.yaml"], input=b"cesarops\n")
subprocess.run(["sudo", "-S", "rm", "-f", "/etc/netplan/50-cloud-init.yaml"], input=b"cesarops\n")

# Remove old config that only has eno1
old = pathlib.Path("/etc/netplan/00-installer-config.yaml")
if old.exists():
    subprocess.run(["sudo", "-S", "rm", "-f", str(old)], input=b"cesarops\n")

print("Netplan written. Applying...")
result = subprocess.run(["sudo", "-S", "netplan", "apply"], input=b"cesarops\n", capture_output=True, text=True)
print(result.stdout)
if result.returncode != 0:
    print(f"Error: {result.stderr}")
    print("WARNING: If connection drops, the bond may need both cables plugged in.")
    print("Fallback: sudo netplan apply with just eno1 config")
else:
    print("Bond active. Check: ip addr show bond0")
