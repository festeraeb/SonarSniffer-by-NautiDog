use anyhow::Result;
use nauticuvs::protocol::{NodeCapabilities, NodeRole};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

// ── Hardware capability classification ───────────────────────────────────────

/// Distinguishes true FP64 silicon from consumer cards that technically
/// support FP64 but at 1:32 throughput — unusable for precision curvelet math.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fp64Capability {
    /// 1:2 FP64:FP32 rate — Tesla P100, V100, A100, Xeon AVX-512.
    /// Safe to route nauticuvs f64 curvelet passes here.
    NativeFull,
    /// 1:32 FP64:FP32 rate — Consumer Pascal (1070, 1060), Quadro P1000 (GP107).
    /// Technically capable but 32× slower than FP32 — do NOT route precision math here.
    EmulatedSlow,
}

/// Full hardware signature for a compute device (GPU or CPU).
#[derive(Debug, Clone)]
pub struct DeviceHardwareSignature {
    pub name: String,
    /// True only for NativeFull devices — gates routing of f64 curvelet passes.
    pub has_fp64: bool,
    pub fp64_rate: Fp64Capability,
    /// AVX-512F detected — Xeon Silver 4110 supports this, enables 8×f64 SIMD lanes.
    pub native_avx512: bool,
}

impl DeviceHardwareSignature {
    /// Evaluate hardware capability from device name.
    /// Prevents routing high-precision curvelet loops to weak FP64 silicon.
    pub fn evaluate(name: &str, is_cpu: bool) -> Self {
        if is_cpu {
            // Xeon Silver 4110 — full native FP64, check for AVX-512F at runtime.
            // AVX-512 gives 8 doubles/cycle per core — critical for curvelet wrapping.
            let has_avx512 = is_x86_feature_detected!("avx512f");
            info!("CPU: {} | FP64: NativeFull | AVX-512: {}", name, has_avx512);
            return Self {
                name: name.to_string(),
                has_fp64: true,
                fp64_rate: Fp64Capability::NativeFull,
                native_avx512: has_avx512,
            };
        }

        let n = name.to_uppercase();

        // True FP64 GPUs: Tesla P100 (GP100 die), V100, A100, A40.
        // Explicitly exclude T4 (Turing, 1:32 FP64) and all consumer Pascal.
        // Quadro P1000 uses GP107 die — same 1:32 rate as GTX 1050.
        let is_true_fp64 = n.contains("TESLA P100")
            || n.contains("TESLA P40")
            || n.contains("TESLA V100")
            || n.contains("A100")
            || n.contains("A40")
            || (n.contains("QUADRO") && n.contains("GP100")); // Quadro GP100 only

        let fp64_rate = if is_true_fp64 {
            Fp64Capability::NativeFull
        } else {
            Fp64Capability::EmulatedSlow
        };

        if !is_true_fp64 && (n.contains("QUADRO") || n.contains("TESLA")) {
            warn!(
                "GPU '{}' is Quadro/Tesla but NOT true FP64 (GP107/Turing die) — \
                 routing to EmulatedSlow. Use P100/V100/A100 for curvelet f64 passes.",
                name
            );
        }

        Self {
            name: name.to_string(),
            has_fp64: is_true_fp64,
            fp64_rate,
            native_avx512: false,
        }
    }

    /// Returns true if this device should handle nauticuvs f64 curvelet passes.
    pub fn can_run_precision_math(&self) -> bool {
        self.fp64_rate == Fp64Capability::NativeFull
    }
}

// ── AllocationEngine ──────────────────────────────────────────────────────────

pub struct AllocationEngine {
    pub capabilities: Arc<RwLock<NodeCapabilities>>,
}

impl AllocationEngine {
    pub async fn detect() -> Result<Self> {
        let node_id = hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());

        let (total_vram_gb, available_vram_gb, gpu_name, has_fp64) = Self::query_hardware();
        let has_tpu = Self::detect_tpu();

        // Also evaluate CPU capability — Xeon Silver 4110 with AVX-512 is a
        // first-class compute device for sequential FP64 curvelet math.
        let cpu_name = Self::cpu_model_name();
        let cpu_sig = DeviceHardwareSignature::evaluate(&cpu_name, true);
        if cpu_sig.native_avx512 {
            info!("CPU: {} | AVX-512F detected — eligible for f64 curvelet passes", cpu_name);
        }

        let caps = NodeCapabilities {
            node_id,
            total_vram_gb,
            available_vram_gb,
            has_fp64,
            has_tpu,
            gpu_name,
        };

        info!(
            "Hardware: {} | GPU: {} | VRAM: {}GB total / {}GB free | FP64: {} | TPU: {} | CPU AVX-512: {}",
            caps.node_id, caps.gpu_name, caps.total_vram_gb, caps.available_vram_gb,
            caps.has_fp64, caps.has_tpu, cpu_sig.native_avx512
        );

        Ok(Self {
            capabilities: Arc::new(RwLock::new(caps)),
        })
    }

    fn cpu_model_name() -> String {
        // Read from /proc/cpuinfo on Linux — no external deps
        #[cfg(target_os = "linux")]
        {
            if let Ok(info) = std::fs::read_to_string("/proc/cpuinfo") {
                for line in info.lines() {
                    if line.starts_with("model name") {
                        if let Some(name) = line.split(':').nth(1) {
                            return name.trim().to_string();
                        }
                    }
                }
            }
        }
        "Unknown CPU".to_string()
    }

    fn query_hardware() -> (u32, u32, String, bool) {
        if let Ok(result) = Self::query_nvml() {
            return result;
        }
        Self::query_wgpu_adapters().unwrap_or_else(|| {
            warn!("No GPU detected — running CPU-only mode");
            (0, 0, "CPU-only".to_string(), false)
        })
    }

    fn query_nvml() -> Result<(u32, u32, String, bool)> {
        use nvml_wrapper::Nvml;
        let nvml = Nvml::init()?;
        let device = nvml.device_by_index(0)?;

        let mem = device.memory_info()?;
        let total_gb = (mem.total / (1024 * 1024 * 1024)) as u32;
        let avail_gb = (mem.free / (1024 * 1024 * 1024)) as u32;
        let name = device.name()?;

        let sig = DeviceHardwareSignature::evaluate(&name, false);
        info!("NVML: {} | {}GB total / {}GB free | FP64: {:?}", name, total_gb, avail_gb, sig.fp64_rate);
        Ok((total_gb, avail_gb, name, sig.has_fp64))
    }

    fn query_wgpu_adapters() -> Option<(u32, u32, String, bool)> {
        use wgpu::{Instance, PowerPreference, RequestAdapterOptions};

        let instance = Instance::default();
        let adapter = pollster::block_on(instance.request_adapter(&RequestAdapterOptions {
            power_preference: PowerPreference::HighPerformance,
            compatible_surface: None,
            force_fallback_adapter: false,
        })).ok()?;

        let info = adapter.get_info();
        let gpu_name = info.name.clone();
        let vram_gb = Self::estimate_vram_from_name(&gpu_name);
        let sig = DeviceHardwareSignature::evaluate(&gpu_name, false);

        warn!("NVML unavailable — wgpu fallback: {} (~{}GB estimated VRAM) | FP64: {:?}",
            gpu_name, vram_gb, sig.fp64_rate);
        Some((vram_gb, vram_gb, gpu_name, sig.has_fp64))
    }

    fn estimate_vram_from_name(name: &str) -> u32 {
        let n = name.to_uppercase();
        if n.contains("P40") { 24 }
        else if n.contains("P100") { 16 }
        else if n.contains("P4") { 8 }
        else if n.contains("1080 TI") || n.contains("1080TI") { 11 }
        else if n.contains("1080") { 8 }
        else if n.contains("1070") { 8 }
        else if n.contains("1060") { 6 }
        else if n.contains("3090") { 24 }
        else if n.contains("3080 TI") || n.contains("3080TI") { 12 }
        else if n.contains("3080") { 10 }
        else if n.contains("4090") { 24 }
        else if n.contains("4080") { 16 }
        else if n.contains("4070 TI") || n.contains("4070TI") { 12 }
        else { 4 }
    }

    fn detect_tpu() -> bool {
        #[cfg(target_os = "linux")]
        {
            std::path::Path::new("/dev/apex_0").exists()
                || std::path::Path::new("/dev/accel0").exists()
        }

        #[cfg(target_os = "windows")]
        {
            if std::env::var("EDGETPU_PRESENT").as_deref().map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
                || std::env::var("TPU_PRESENT").as_deref().map(|v| v == "1" || v.eq_ignore_ascii_case("true")).unwrap_or(false)
            {
                return true;
            }

            Self::find_library_in_path("edgetpu.dll") || Self::find_library_in_path("libedgetpu.dll")
        }

        #[cfg(not(any(target_os = "linux", target_os = "windows")))]
        {
            false
        }
    }

    #[cfg(target_os = "windows")]
    fn find_library_in_path(file_name: &str) -> bool {
        if let Some(paths) = std::env::var_os("PATH") {
            for path in std::env::split_paths(&paths) {
                let candidate = path.join(file_name);
                if candidate.exists() {
                    return true;
                }
            }
        }
        false
    }

    pub async fn determine_role(&self) -> NodeRole {
        let caps = self.capabilities.read().await;
        match caps.total_vram_gb {
            v if v >= 24 => NodeRole::SuperAgent,
            v if v >= 8 && caps.has_fp64 => NodeRole::Analyst,
            v if v >= 12 => NodeRole::CoderReasoningLead,
            v if v >= 8 => NodeRole::CoderReasoningLead,
            v if v >= 6 => NodeRole::CoderExecutionWorker,
            _ if caps.has_tpu => NodeRole::Scout,
            _ => NodeRole::Idle,
        }
    }

    pub async fn can_accept(&self, required_vram_gb: u32, required_fp64: bool, requires_tpu: bool) -> bool {
        let caps = self.capabilities.read().await;
        caps.available_vram_gb >= required_vram_gb
            && (!required_fp64 || caps.has_fp64)
            && (!requires_tpu || caps.has_tpu)
    }

    pub async fn reserve(&self, vram_gb: u32) {
        let mut caps = self.capabilities.write().await;
        caps.available_vram_gb = caps.available_vram_gb.saturating_sub(vram_gb);
    }

    pub async fn release(&self, vram_gb: u32) {
        let mut caps = self.capabilities.write().await;
        caps.available_vram_gb = (caps.available_vram_gb + vram_gb).min(caps.total_vram_gb);
    }
}
