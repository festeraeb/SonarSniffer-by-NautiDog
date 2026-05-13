#!/usr/bin/env python3
"""
CESAROPS Credential Inventory Scanner
Runs locally on cesarops3 (P106 node).
Scans the codebase for credential references and produces an inventory.
DOES NOT extract or copy actual secret values — only maps what exists and where.
Output: /tmp/credential_inventory.json
"""

import os
import re
import json
from pathlib import Path
from datetime import datetime

# Scan target — mount the RAID share or use a local copy
SCAN_DIRS = [
    "/home/cesarops/wreckhunter2000-1",  # local if synced
    "/mnt/codebase/wreckhunter2000-1",   # if RAID is mounted via Samba
]

# Patterns that indicate credentials (we find WHERE they are, not WHAT they contain)
PATTERNS = {
    "env_var": re.compile(r'(?:std::env::var|os\.environ|os\.getenv|env::var)\s*\(\s*["\']([A-Z_]+)["\']'),
    "api_key_assignment": re.compile(r'(?:api_key|API_KEY|apikey|secret|token|password|passwd)\s*[:=]\s*["\']?([^\s"\']+)'),
    "dotenv_ref": re.compile(r'([A-Z][A-Z0-9_]+)\s*='),
    "bearer_token": re.compile(r'[Bb]earer\s+\S+'),
    "url_with_auth": re.compile(r'https?://[^:]+:[^@]+@'),
    "ssh_key_ref": re.compile(r'(?:id_rsa|id_ed25519|\.pem|\.key)'),
    "hardcoded_ip_port": re.compile(r'\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3}:\d{2,5}'),
}

# Files to specifically check
CREDENTIAL_FILES = [
    ".env", ".env.example", ".env.local",
    "credentials.sh", "secrets.toml", "secrets.json",
    "cloudflared_config.yml", "smb.conf",
    "*.service",  # systemd files often have paths/configs
]

# Skip these directories
SKIP_DIRS = {".git", "target", "node_modules", ".venv", "__pycache__", ".cargo"}

# Skip binary files
BINARY_EXTENSIONS = {".gguf", ".bin", ".so", ".o", ".a", ".pyc", ".db", ".tif", ".tiff", ".png", ".jpg"}


def find_scan_dir():
    for d in SCAN_DIRS:
        if os.path.isdir(d):
            return d
    return None


def scan_file(filepath):
    """Scan a single file for credential patterns. Returns findings."""
    findings = []
    try:
        with open(filepath, 'r', errors='ignore') as f:
            for line_num, line in enumerate(f, 1):
                for pattern_name, pattern in PATTERNS.items():
                    matches = pattern.findall(line)
                    if matches:
                        for match in matches:
                            # Don't include the actual value — just the key name and location
                            findings.append({
                                "file": str(filepath),
                                "line": line_num,
                                "type": pattern_name,
                                "key_name": match if pattern_name in ("env_var", "dotenv_ref") else "[redacted]",
                                "context": line.strip()[:80] + "..." if len(line.strip()) > 80 else line.strip(),
                            })
    except (PermissionError, OSError):
        pass
    return findings


def scan_directory(root_dir):
    """Walk the directory tree and scan all text files."""
    all_findings = []
    credential_files_found = []
    
    for dirpath, dirnames, filenames in os.walk(root_dir):
        # Skip excluded directories
        dirnames[:] = [d for d in dirnames if d not in SKIP_DIRS]
        
        rel_dir = os.path.relpath(dirpath, root_dir)
        
        for filename in filenames:
            filepath = Path(dirpath) / filename
            
            # Skip binary files
            if filepath.suffix.lower() in BINARY_EXTENSIONS:
                continue
            
            # Check if this is a known credential file
            for cred_pattern in CREDENTIAL_FILES:
                if cred_pattern.startswith("*"):
                    if filename.endswith(cred_pattern[1:]):
                        credential_files_found.append(str(filepath))
                elif filename == cred_pattern:
                    credential_files_found.append(str(filepath))
            
            # Scan text files under 1MB
            if filepath.stat().st_size < 1_000_000:
                findings = scan_file(filepath)
                all_findings.extend(findings)
    
    return all_findings, credential_files_found


def generate_inventory(findings, credential_files, scan_dir):
    """Generate the inventory report."""
    # Group findings by type
    by_type = {}
    for f in findings:
        t = f["type"]
        if t not in by_type:
            by_type[t] = []
        by_type[t].append(f)
    
    # Extract unique env var names
    env_vars = sorted(set(
        f["key_name"] for f in findings 
        if f["type"] in ("env_var", "dotenv_ref") and f["key_name"] != "[redacted]"
    ))
    
    # Extract unique service endpoints
    endpoints = sorted(set(
        f["key_name"] for f in findings
        if f["type"] == "hardcoded_ip_port"
    ))
    
    inventory = {
        "generated_at": datetime.now().isoformat(),
        "scan_directory": scan_dir,
        "total_findings": len(findings),
        "credential_files_found": credential_files,
        "environment_variables_referenced": env_vars,
        "service_endpoints_found": len([f for f in findings if f["type"] == "hardcoded_ip_port"]),
        "summary_by_type": {k: len(v) for k, v in by_type.items()},
        "findings": findings[:500],  # Cap at 500 to keep file manageable
    }
    
    return inventory


def main():
    scan_dir = find_scan_dir()
    if not scan_dir:
        print("ERROR: No scan directory found. Mount the RAID or sync the repo.")
        return
    
    print(f"Scanning: {scan_dir}")
    print("Looking for credential references (NOT extracting values)...")
    
    findings, credential_files = scan_directory(scan_dir)
    
    print(f"Found {len(findings)} credential references in {len(credential_files)} credential files")
    
    inventory = generate_inventory(findings, credential_files, scan_dir)
    
    output_path = "/tmp/credential_inventory.json"
    with open(output_path, 'w') as f:
        json.dump(inventory, f, indent=2)
    
    print(f"Inventory written to: {output_path}")
    print(f"Environment variables referenced: {len(inventory['environment_variables_referenced'])}")
    print(f"Credential files found: {len(credential_files)}")
    for cf in credential_files:
        print(f"  - {cf}")
    print(f"\nEnv vars found:")
    for ev in inventory['environment_variables_referenced'][:20]:
        print(f"  - {ev}")


if __name__ == "__main__":
    main()
