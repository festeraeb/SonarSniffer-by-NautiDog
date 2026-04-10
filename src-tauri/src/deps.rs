use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DependencyStatus {
    pub gstreamer_available: bool,
    pub gstreamer_version: Option<String>,
    pub message: String,
}

pub fn check_gstreamer() -> DependencyStatus {
    // Try to detect GStreamer via gst crate or PATH
    match std::process::Command::new("gst-launch-1.0")
        .arg("--version")
        .output()
    {
        Ok(output) if output.status.success() => {
            let version = String::from_utf8_lossy(&output.stdout)
                .lines()
                .next()
                .unwrap_or("")
                .to_string();
            DependencyStatus {
                gstreamer_available: true,
                gstreamer_version: Some(version),
                message: "GStreamer is available".into(),
            }
        }
        _ => DependencyStatus {
            gstreamer_available: false,
            gstreamer_version: None,
            message: "GStreamer not found. Video export requires GStreamer runtime.".into(),
        },
    }
}

pub fn install_gstreamer_runtime() -> Result<String, String> {
    Err("Automatic GStreamer installation not supported. Please download from https://gstreamer.freedesktop.org/download/".into())
}
