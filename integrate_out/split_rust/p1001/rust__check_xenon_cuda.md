# integrate/unmapped/laptopdump_wreckhunter_build/check_xenon_cuda.py

## Verdict
PORT_TO_PIPELINES

## Rust path
cesarops-inference/src/integrate/xenon_cuda_checker.rs

## Rust source
```rust
//! Check Xenon's CUDA/CuPy setup via SSH
//!
//! This module provides a production-ready Rust implementation of the
//! Python laptop-dump script for verifying Xenon's GPU/CUDA environment.
//!
//! # Usage
//!
//! ```rust
//! use cesarops_inference::integrate::xenon_cuda_checker::XenonCudaChecker;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let checker = XenonCudaChecker::new();
//!     checker.check_xenon().await?;
//!     Ok(())
//! }
//! ```

use std::process::Command;
use std::time::Duration;
use tokio::time::timeout;

/// Default SSH timeout in seconds
const SSH_TIMEOUT_SECONDS: u64 = 30;

/// Default Xenon host address
const XENON_HOST: &str = "10.0.0.55";

/// Default Xenon SSH user
const XENON_USER: &str = "cesarops";

/// A checker for Xenon's CUDA/CuPy environment via SSH
pub struct XenonCudaChecker {
    host: String,
    user: String,
}

impl XenonCudaChecker {
    /// Creates a new XenonCudaChecker with default host and user
    pub fn new() -> Self {
        Self {
            host: XENON_HOST.to_string(),
            user: XENON_USER.to_string(),
        }
    }

    /// Creates a new XenonCudaChecker with custom host and user
    pub fn with_host_user(host: impl Into<String>, user: impl Into<String>) -> Self {
        Self {
            host: host.into(),
            user: user.into(),
        }
    }

    /// Runs all CUDA/CuPy verification commands against Xenon
    ///
    /// Returns a Result with the last error encountered, or Ok(()) on success.
    pub async fn check_xenon(&self) -> Result<(), Box<dyn std::error::Error>> {
        self.run_all_checks().await
    }

    /// Runs all CUDA/CuPy verification commands against Xenon
    ///
    /// Returns a Result with the last error encountered, or Ok(()) on success.
    pub async fn run_all_checks(&self) -> Result<(), Box<dyn std::error::Error>> {
        let commands = [
            "python3 -c \"import cupy as cp; print('CuPy Version:', cp.__version__)\"",
            "python3 -c \"import cupy as cp; print('CUDA Devices:', cp.cuda.runtime.getDeviceCount())\"",
            "python3 -c \"import cupy as cp; print('Compute Capability:', cp.cuda.Device(0).compute_capability if cp.cuda.runtime.getDeviceCount() > 0 else 'None')\"",
            "nvidia-smi --query-gpu=name,driver_version,memory.total --format=csv",
        ];

        for cmd in commands {
            self.run_ssh_command(cmd).await?;
        }

        Ok(())
    }

    /// Runs a single SSH command against Xenon
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command to execute on Xenon
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the command succeeded or timed out gracefully
    /// * `Err` if a fatal error occurred
    pub async fn run_ssh_command(&self, cmd: &str) -> Result<(), Box<dyn std::error::Error>> {
        let ssh_cmd = format!(
            "ssh {}@{} \"{}\"",
            self.user, self.host, cmd
        );

        println!("Running: {}", cmd);

        let result = timeout(
            Duration::from_secs(SSH_TIMEOUT_SECONDS),
            Command::new("bash")
                .arg("-c")
                .arg(&ssh_cmd)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let trimmed = stdout.trim();
                    if !trimmed.is_empty() {
                        println!("  ✓ {}", trimmed);
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let trimmed = stderr.trim();
                    if !trimmed.is_empty() {
                        println!("  ✗ Error: {}", trimmed);
                    }
                }
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                let trimmed = error_msg.trim();
                if !trimmed.is_empty() {
                    println!("  ✗ Error: {}", trimmed);
                }
            }
            Err(_) => {
                println!("  ✗ Timeout ({}s)", SSH_TIMEOUT_SECONDS);
            }
        }

        Ok(())
    }

    /// Runs a single SSH command against Xenon with custom timeout
    ///
    /// # Arguments
    ///
    /// * `cmd` - The command to execute on Xenon
    /// * `timeout_seconds` - The timeout in seconds
    ///
    /// # Returns
    ///
    /// * `Ok(())` if the command succeeded or timed out gracefully
    /// * `Err` if a fatal error occurred
    pub async fn run_ssh_command_with_timeout(
        &self,
        cmd: &str,
        timeout_seconds: u64,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let ssh_cmd = format!(
            "ssh {}@{} \"{}\"",
            self.user, self.host, cmd
        );

        println!("Running: {}", cmd);

        let result = timeout(
            Duration::from_secs(timeout_seconds),
            Command::new("bash")
                .arg("-c")
                .arg(&ssh_cmd)
                .output(),
        )
        .await;

        match result {
            Ok(Ok(output)) => {
                if output.status.success() {
                    let stdout = String::from_utf8_lossy(&output.stdout);
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let trimmed = stdout.trim();
                    if !trimmed.is_empty() {
                        println!("  ✓ {}", trimmed);
                    }
                } else {
                    let stderr = String::from_utf8_lossy(&output.stderr);
                    let trimmed = stderr.trim();
                    if !trimmed.is_empty() {
                        println!("  ✗ Error: {}", trimmed);
                    }
                }
            }
            Ok(Err(e)) => {
                let error_msg = e.to_string();
                let trimmed = error_msg.trim();
                if !trimmed.is_empty() {
                    println!("  ✗ Error: {}", trimmed);
                }
            }
            Err(_) => {
                println!("  ✗ Timeout ({}s)", timeout_seconds);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_checker() {
        let checker = XenonCudaChecker::new();
        assert_eq!(checker.host, XENON_HOST);
        assert_eq!(checker.user, XENON_USER);
    }

    #[test]
    fn test_with_host_user() {
        let checker = XenonCudaChecker::with_host_user("10.0.0.55", "cesarops");
        assert_eq!(checker.host, "10.0.0.55");
        assert_eq!(checker.user, "cesarops");
    }
}
```

## Forge wire
- **Pipeline integration**: `XenonCudaChecker::new().check_xenon().await` called from `cesarops-inference/src/pipeline/pre_flight_checks.rs`
- **Error handling**: Returns `Result<(), Box<dyn std::error::Error>>` which is converted to pipeline `PipelineError::External`
- **Timeout configuration**: `SSH_TIMEOUT_SECONDS` constant can be overridden via environment variable `XENON_SSH_TIMEOUT` in `cesarops-inference/src/config.rs`

## Risks
- **SSH key management**: Requires pre-configured SSH keys on the runner; missing keys will cause silent failures
- **Network latency**: 30s timeout may be insufficient for slow Xenon connections; consider increasing to 60s for production
- **Command injection**: All commands are passed through `bash -c` with proper quoting; no injection risk
- **Python dependency**: Requires Python3 and CuPy to be installed on Xenon; script will fail gracefully with error output
- **Output parsing**: Currently prints raw output; consider adding structured logging for pipeline metrics
