use std::fs;
use std::path::Path;
use std::process::Command;

use crate::config::{DriveMount, MountType};

/// Check if a mount point is currently active.
/// Returns true if the path exists and is a mount point.
fn is_mounted(mount_point: &str) -> bool {
    let path = Path::new(mount_point);
    if !path.exists() {
        return false;
    }
    // Simple check: try to read /proc/mounts or use statfs. 
    // For robustness in a supervisor, we check if the directory is a mount point.
    // A reliable way in Rust without external crates is checking /proc/self/mountinfo,
    // but for simplicity and following "no unwraps", we'll use a heuristic:
    // Check if the directory is empty? No, that's not reliable.
    // We will rely on the fact that if we successfully stat the mount point and it's a directory,
    // we assume it's mounted unless we detect otherwise via error on access.
    // However, to be precise, let's check /proc/mounts.
    if let Ok(content) = fs::read_to_string("/proc/self/mountinfo") {
        for line in content.lines() {
            if line.contains(mount_point) {
                // Ensure it's the root of the mount (not a subdirectory)
                // Format: id parent_id major:minor root mountpoint options ...
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 5 && parts[4] == mount_point {
                    return true;
                }
            }
        }
    }
    false
}

/// Ensure a drive is mounted. If not, mount it.
pub fn ensure_mount(drive: &DriveMount) -> Result<(), String> {
    let mount_point = &drive.mount_point;
    
    if is_mounted(mount_point) {
        return Ok(());
    }

    // Create mount point if it doesn't exist
    if !Path::new(mount_point).exists() {
        fs::create_dir_all(mount_point).map_err(|e| format!("Failed to create mount point {}: {}", mount_point, e))?;
    }

    let mut cmd = Command::new("mount");
    
    match &drive.mount_type {
        MountType::Uuid { uuid, fs_type } => {
            cmd.arg("-t")
               .arg(fs_type)
               .arg(format!("UUID={}", uuid))
               .arg(mount_point);
        }
        MountType::Device { device, fs_type } => {
            cmd.arg("-t")
               .arg(fs_type)
               .arg(device)
               .arg(mount_point);
        }
        MountType::Cifs { server, share } => {
            cmd.arg("-t")
               .arg("cifs")
               .arg(format!("//{}/{}", server, share))
               .arg(mount_point);
            // Note: In a real scenario, we might need credentials. 
            // Assuming /etc/fstab handles auth or no-auth is configured.
        }
    }

    let output = cmd.output().map_err(|e| format!("Failed to execute mount command for {}: {}", mount_point, e))?;
    
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("Mount failed for {}: {}", mount_point, stderr));
    }

    // Verify it's mounted after the command
    if !is_mounted(mount_point) {
        return Err(format!("Mount command succeeded but {} is not mounted", mount_point));
    }

    Ok(())
}

/// Check all configured drives and mount them if missing.
pub fn ensure_all_mounts(drives: &[DriveMount]) -> Result<(), String> {
    for drive in drives {
        ensure_mount(drive)?;
    }
    Ok(())
}
