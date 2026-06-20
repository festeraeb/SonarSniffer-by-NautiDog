#![cfg_attr(not(windows), allow(unused_imports))]
mod home_forge_tunnel;

use std::fs::{self, File};
use std::io::copy;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::thread;
use std::time::Duration;

#[cfg(not(windows))]
fn main() {
    eprintln!("SonarSniffer-Setup.exe is Windows-only. Build with:");
    eprintln!("  SONARSNIFFER_SETUP_PAYLOAD=path/to/payload.zip cargo build --release");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(e) = run() {
        eprintln!("SonarSniffer setup failed: {e}");
        report_telemetry("install_error", &e.to_string());
        pause_on_error();
        std::process::exit(1);
    }
}

#[cfg(windows)]
fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("SonarSniffer — preparing install...\n");

    let staging = extract_payload()?;
    let install_ps1 = staging.join("install_sonarsniffer_full.ps1");
    if !install_ps1.is_file() {
        return Err(format!(
            "missing install script in kit: {}",
            install_ps1.display()
        )
        .into());
    }

    println!("Launching PowerShell installer (Administrator)...\n");
    launch_powershell_elevated(&install_ps1, &staging)?;
    Ok(())
}

#[cfg(windows)]
fn extract_payload() -> Result<PathBuf, Box<dyn std::error::Error>> {
    let base = std::env::temp_dir().join(format!(
        "SonarSniffer-Setup-{}",
        std::process::id()
    ));
    fs::create_dir_all(&base)?;

    #[cfg(embed_payload)]
    {
        const PAYLOAD: &[u8] = include!(concat!(env!("OUT_DIR"), "/embedded_payload.rs"));
        extract_zip(PAYLOAD, &base)?;
        return Ok(base);
    }

    #[cfg(not(embed_payload))]
    {
        let exe_dir = std::env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
        let sidecar = exe_dir.join("SonarSniffer-Setup-payload.zip");
        if sidecar.is_file() {
            let data = fs::read(&sidecar)?;
            extract_zip(&data, &base)?;
            return Ok(base);
        }
        Err(
            "No embedded payload. Build with SONARSNIFFER_SETUP_PAYLOAD or place SonarSniffer-Setup-payload.zip next to this exe.".into(),
        )
    }
}

fn extract_zip(data: &[u8], dest: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let cursor = std::io::Cursor::new(data);
    let mut archive = zip::ZipArchive::new(cursor)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let name = file.name().replace('\\', "/");
        if name.ends_with('/') {
            fs::create_dir_all(dest.join(&name))?;
            continue;
        }
        let out_path = dest.join(&name);
        if let Some(parent) = out_path.parent() {
            fs::create_dir_all(parent)?;
        }
        if file.is_dir() {
            fs::create_dir_all(&out_path)?;
            continue;
        }
        let mut out = File::create(&out_path)?;
        copy(&mut file, &mut out)?;
    }
    Ok(())
}

#[cfg(windows)]
fn launch_powershell_elevated(
    script: &Path,
    workdir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let ps = powershell_path();
    let wd = workdir.to_string_lossy().replace('\'', "''");
    let scr = script.to_string_lossy().replace('\'', "''");
    
    let completed = Arc::new(AtomicBool::new(false));
    let completed_clone = completed.clone();
    
    // Spawn stall watchdog
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(15));
        if !completed_clone.load(Ordering::SeqCst) {
            let payload = serde_json::json!({
                "event": "uac_stall",
                "details": "Elevation prompt stalled for over 15 seconds.",
                "os": "windows"
            });
            report_telemetry("uac_stall", "Elevation prompt stalled for over 15 seconds.");
            let _ = home_forge_tunnel::run_diagnostic_tunnel(payload);
        }
    });

    let elevate_cmd = format!(
        "Start-Process -FilePath '{ps}' -Verb RunAs -Wait -ArgumentList @(
            '-NoProfile','-ExecutionPolicy','Bypass',
            '-WorkingDirectory','{wd}',
            '-File','{scr}'
        )"
    );
    let status = Command::new(&ps)
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &elevate_cmd])
        .status()?;
    
    completed.store(true, Ordering::SeqCst);
    
    if !status.success() {
        return Err(format!("elevated PowerShell launch failed: {status}").into());
    }
    Ok(())
}

#[cfg(windows)]
fn report_telemetry(event: &str, details: &str) {
    let payload = serde_json::json!({
        "event": event,
        "details": details,
        "os": "windows",
        "timestamp": std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    });
    
    // Fire and forget
    let _ = ureq::post("https://api.cesarops.com/telemetry/install")
        .set("Content-Type", "application/json")
        .timeout(std::time::Duration::from_secs(3))
        .send_json(payload);
}

#[cfg(windows)]
fn powershell_path() -> String {
    std::env::var("SystemRoot")
        .map(|w| format!("{w}\\System32\\WindowsPowerShell\\v1.0\\powershell.exe"))
        .unwrap_or_else(|_| "powershell.exe".to_string())
}

#[cfg(windows)]
fn pause_on_error() {
    println!("\nPress Enter to close...");
    let mut buf = String::new();
    let _ = std::io::stdin().read_line(&mut buf);
}
