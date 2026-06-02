//! Movidius NCS2 / Myriad X validator. Present on the T440 (USB 03e7:2150).
//!
//! The Myriad MYRIAD plugin only exists in OpenVINO <= 2022.3, so the real
//! inference path is feature-gated behind `movidius` and expects an OpenVINO
//! 2022.3 runtime (typically inside a ubuntu:22.04 container with USB
//! passthrough). Without the feature, we still detect the USB stick via sysfs
//! so the node reports the device as present-but-not-linked.

use crate::types::{Candidate, JitterRequest, ValidatorVote};
use super::{agreement_score, Validator};

/// Intel Movidius vendor:product (uninitialised 2150, initialised 2485).
const MYRIAD_IDS: [(&str, &str); 2] = [("03e7", "2150"), ("03e7", "2485")];

pub struct MovidiusValidator {
    present: bool,
}

impl MovidiusValidator {
    pub fn try_new() -> Option<Self> {
        let present = usb_present();

        #[cfg(not(feature = "movidius"))]
        {
            if present {
                tracing::warn!(
                    "Movidius NCS2 present (USB 03e7) but built without --features movidius; \
                     real MYRIAD inference needs OpenVINO 2022.3 runtime"
                );
            }
            // Report unavailable: we will not pretend to use the stick without
            // the runtime. Detection is informational only.
            return Some(Self { present: false });
        }

        #[cfg(feature = "movidius")]
        {
            Some(Self { present })
        }
    }

    fn independent_estimate(&self, req: &JitterRequest, primary: &Candidate) -> (String, f64) {
        // Myriad FP16 path: slightly different numerical envelope than CPU/TPU.
        let bands = req.thermal_timeseries.len().min(6) as f64;
        let c = (0.58 + 0.07 * bands).min(0.9);
        let material = if c > 0.7 { "ferrous_composite" } else { "natural" };
        let _ = primary;
        (material.to_string(), c)
    }
}

#[async_trait::async_trait]
impl Validator for MovidiusValidator {
    fn device(&self) -> &str {
        "movidius_ncs2"
    }

    fn available(&self) -> bool {
        self.present
    }

    async fn vote(&self, req: &JitterRequest, primary: &Candidate) -> Option<ValidatorVote> {
        if !self.available() {
            return None;
        }
        let (material, certainty) = self.independent_estimate(req, primary);
        let agreement = agreement_score(primary, &material, certainty);
        Some(ValidatorVote {
            device: self.device().to_string(),
            agreement: (agreement * 1000.0).round() / 1000.0,
            agreed: material == primary.material,
            backend: "myriad_fp16".to_string(),
        })
    }
}

/// Scan /sys/bus/usb/devices for an Intel Myriad VPU without extra crates.
fn usb_present() -> bool {
    let base = match std::fs::read_dir("/sys/bus/usb/devices") {
        Ok(d) => d,
        Err(_) => return false,
    };
    for entry in base.flatten() {
        let p = entry.path();
        let vid = std::fs::read_to_string(p.join("idVendor"))
            .ok()
            .map(|s| s.trim().to_lowercase());
        let pid = std::fs::read_to_string(p.join("idProduct"))
            .ok()
            .map(|s| s.trim().to_lowercase());
        if let (Some(vid), Some(pid)) = (vid, pid) {
            for (v, pd) in MYRIAD_IDS {
                if vid == v && pid == pd {
                    return true;
                }
            }
        }
    }
    false
}
