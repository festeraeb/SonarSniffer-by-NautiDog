use std::fs;
use std::process::Command;

use crate::config::{NetworkService, ServiceType};

/// Check if a network interface is up and has an IP address.
fn is_interface_up(interface: &str) -> bool {
    // Check if interface exists and is UP
    let output = Command::new("ip")
        .args(["link", "show", interface])
        .output()
        .ok();
    
    let Some(output) = output else { return false; };
    if !output.status.success() { return false; }
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Check for state UP
    if !stdout.contains("state UP") {
        return false;
    }

    // Check for IP address
    let output = Command::new("ip")
        .args(["addr", "show", interface])
        .output()
        .ok();
    
    let Some(output) = output else { return false; }
    let stdout = String::from_utf8_lossy(&output.stdout);
    stdout.contains("inet ")
}

/// Check Tailscale status. Ensure tailscaled is running and tailscale0 is up.
pub fn check_tailscale() -> Result<(), String> {
    // Check if tailscaled is running
    let output = Command::new("systemctl")
        .args(["is-active", "tailscaled"])
        .output()
        .map_err(|e| format!("Failed to check tailscale status: {}", e))?;
    
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    if status != "active" {
        // Try to start it
        let _ = Command::new("systemctl")
            .args(["start", "tailscaled"])
            .output()
            .map_err(|e| format!("Failed to start tailscaled: {}", e))?;
        
        // Wait a moment for interface to come up
        std::thread::sleep(std::time::Duration::from_secs(2));
    }

    // Check if tailscale0 interface is up
    if !is_interface_up("tailscale0") {
        return Err("Tailscale interface tailscale0 is not up".to_string());
    }

    Ok(())
}

/// Check physical NICs (eno1, eno2) are up and have IPs.
pub fn check_nics() -> Result<(), String> {
    let nics = ["eno1", "eno2"];
    for nic in &nics {
        if !is_interface_up(nic) {
            return Err(format!("NIC {} is not up or has no IP", nic));
        }
    }
    Ok(())
}

/// Manage cloudflared service.
pub fn manage_cloudflared(token: &str) -> Result<(), String> {
    // Check if cloudflared is running
    let output = Command::new("systemctl")
        .args(["is-active", "cloudflared"])
        .output()
        .map_err(|e| format!("Failed to check cloudflared status: {}", e))?;
    
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    
    if status != "active" {
        // Start cloudflared with token
        // Note: In a real supervisor, we might manage the process directly rather than systemctl.
        // But assuming systemd unit exists that takes token from env or config.
        // For this implementation, we assume a systemd unit 'cloudflared.service' exists.
        // If not, we might need to start it manually. Let's try systemctl first.
        let _ = Command::new("systemctl")
            .args(["start", "cloudflared"])
            .output()
            .map_err(|e| format!("Failed to start cloudflared: {}", e))?;
    }

    // Verify it's running
    let output = Command::new("systemctl")
        .args(["is-active", "cloudflared"])
        .output()
        .map_err(|e| format!("Failed to check cloudflared status: {}", e))?;
    
    let status = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if status != "active" {
        return Err("Cloudflared failed to start".to_string());
    }

    Ok(())
}

/// Check all network services.
pub fn check_network_services(services: &[NetworkService]) -> Result<(), String> {
    for service in services {
        match service {
            NetworkService::Tailscale => check_tailscale()?,
            NetworkService::Cloudflared { token } => manage_cloudflared(token)?,
            NetworkService::Nics => check_nics()?,
        }
    }
    Ok(())
}
