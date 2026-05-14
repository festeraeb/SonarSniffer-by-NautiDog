#[cfg(feature = "edgetpu")]
use std::ffi::CStr;
use std::ffi::c_char;
use std::sync::{Arc, Mutex};
use tracing::warn;

// FFI bindings to libedgetpu.so (Coral/Google Edge TPU C API)
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeTpuDeviceType {
    ApexPci = 0,
    ApexUsb = 1,
}

#[repr(C)]
pub struct RawEdgeTpuDevice {
    pub device_type: EdgeTpuDeviceType,
    pub path: *const c_char,
}

#[cfg(feature = "edgetpu")]
#[link(name = "edgetpu")]
unsafe extern "C" {
    fn edgetpu_list_devices(num_devices: *mut usize) -> *mut RawEdgeTpuDevice;
    fn edgetpu_free_devices(devices: *mut RawEdgeTpuDevice);
    fn edgetpu_verbosity(verbosity: i32);
}

#[derive(Debug, Clone)]
pub struct EdgeTpuDevice {
    pub device_type: EdgeTpuDeviceType,
    pub path: String,
}

pub struct EdgeTpuContext {
    devices: Vec<EdgeTpuDevice>,
}

impl EdgeTpuContext {
    /// Enumerate all attached Coral Edge TPU devices.
    pub fn init() -> Option<Arc<Mutex<Self>>> {
        if !std::path::Path::new("/dev/apex_0").exists()
            && !std::path::Path::new("/dev/accel0").exists()
        {
            return None;
        }

        #[cfg(not(feature = "edgetpu"))]
        {
            // Device present but not linked — report as available but not enumerated
            warn!("EdgeTPU device found but built without --features edgetpu");
            return None;
        }

        #[cfg(feature = "edgetpu")]
        let devices = unsafe {
            edgetpu_verbosity(0);
            let mut count: usize = 0;
            let raw = edgetpu_list_devices(&mut count);
            if raw.is_null() || count == 0 {
                return None;
            }
            let slice = std::slice::from_raw_parts(raw, count);
            let devs: Vec<EdgeTpuDevice> = slice
                .iter()
                .map(|d| EdgeTpuDevice {
                    device_type: d.device_type,
                    path: CStr::from_ptr(d.path)
                        .to_string_lossy()
                        .into_owned(),
                })
                .collect();
            edgetpu_free_devices(raw);
            devs
        };

        #[cfg(feature = "edgetpu")]
        {
            if devices.is_empty() {
                warn!("EdgeTPU device nodes found but edgetpu_list_devices returned 0");
                return None;
            }
            for d in &devices {
                tracing::info!("EdgeTPU: {:?} @ {}", d.device_type, d.path);
            }
            Some(Arc::new(Mutex::new(Self { devices })))
        }
    }

    pub fn device_count(&self) -> usize {
        self.devices.len()
    }

    pub fn devices(&self) -> &[EdgeTpuDevice] {
        &self.devices
    }

    /// Returns the path of the primary (first PCIe) TPU device.
    pub fn primary_path(&self) -> Option<&str> {
        self.devices
            .iter()
            .find(|d| d.device_type == EdgeTpuDeviceType::ApexPci)
            .or_else(|| self.devices.first())
            .map(|d| d.path.as_str())
    }
}

// Satisfy the unused import if libedgetpu not present at link time on non-i7 nodes.
pub fn try_init() -> Option<Arc<Mutex<EdgeTpuContext>>> {
    // Only attempt on Linux with /dev/apex_* present.
    #[cfg(target_os = "linux")]
    {
        EdgeTpuContext::init()
    }
    #[cfg(not(target_os = "linux"))]
    {
        None
    }
}
