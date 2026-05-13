#!/usr/bin/env bash
# Install KoboldCpp on Linux node

set -e

echo "Installing KoboldCpp..."

# Download latest release
wget -q https://github.com/LostRuins/koboldcpp/releases/latest/download/koboldcpp-linux-x64 -O koboldcpp
chmod +x koboldcpp

# Create directory
mkdir -p /home/cesarops/koboldcpp
mv koboldcpp /home/cesarops/koboldcpp/

echo "KoboldCpp installed."