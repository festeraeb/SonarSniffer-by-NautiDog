//! Cluster inference telemetry — tracks TTFT, decode throughput, and VRAM usage.
//!
//! Critical for MoE models on P100 HBM2 where expert weight paging
//! is the bottleneck, not compute. Monitors for PCIe lane saturation
//! and VRAM overflow conditions.
//!
//! Compile with `--features nvml` to enable live GPU memory tracking.

use std::time::{Duration, Instant};

#[cfg(feature = "nvml")]
use nvml_wrapper::Nvml;

/// Finalized performance metrics from a single inference pass.
#[derive(Debug, Clone)]
pub struct ClusterMetrics {
    pub prompt_tokens: usize,
    pub generated_tokens: usize,
    pub prefill_duration: Duration,
    pub decode_duration: Duration,
    pub starting_vram_used_bytes: u64,
    pub peak_vram_used_bytes: u64,
}

impl ClusterMetrics {
    /// Time-to-First-Token (TTFT) in milliseconds — measures prefill latency.
    pub fn ttft_ms(&self) -> u128 {
        self.prefill_duration.as_millis()
    }

    /// Tokens Per Second (TPS) throughput during active decode phase.
    pub fn tokens_per_second(&self) -> f64 {
        if self.decode_duration.as_secs_f64() > 0.0 {
            self.generated_tokens as f64 / self.decode_duration.as_secs_f64()
        } else {
            0.0
        }
    }

    /// Total end-to-end latency.
    pub fn total_duration(&self) -> Duration {
        self.prefill_duration + self.decode_duration
    }

    /// VRAM allocation delta during reasoning (MoE expert swap overhead).
    pub fn vram_delta_mb(&self) -> f64 {
        if self.peak_vram_used_bytes >= self.starting_vram_used_bytes {
            (self.peak_vram_used_bytes - self.starting_vram_used_bytes) as f64 / 1_048_576.0
        } else {
            0.0
        }
    }

    /// Print a formatted telemetry report.
    pub fn print_report(&self, role: &str) {
        println!("📊 CLUSTER INFERENCE TELEMETRY [{}]", role);
        println!("═══════════════════════════════════════════════");
        println!("  Prompt:     {} tokens", self.prompt_tokens);
        println!("  Generated:  {} tokens", self.generated_tokens);
        println!("  TTFT:       {} ms", self.ttft_ms());
        println!("  Throughput: {:.1} tok/s", self.tokens_per_second());
        println!("  Total:      {:.2}s", self.total_duration().as_secs_f64());

        if self.starting_vram_used_bytes > 0 {
            println!("  VRAM Base:  {:.0} MB", self.starting_vram_used_bytes as f64 / 1_048_576.0);
            println!("  VRAM Peak:  {:.0} MB", self.peak_vram_used_bytes as f64 / 1_048_576.0);
            println!("  MoE Delta:  +{:.1} MB", self.vram_delta_mb());
        } else {
            println!("  VRAM:       disabled (compile with --features nvml)");
        }

        println!("═══════════════════════════════════════════════");

        if self.ttft_ms() > 1500 {
            eprintln!("⚠️ High TTFT — check prompt size or PCIe bandwidth");
        }
        if self.tokens_per_second() < 12.0 && self.generated_tokens > 10 {
            eprintln!("⚠️ Low throughput — MoE expert swaps may be overflowing VRAM");
        }
    }
}

/// Live tracker that measures inference timing during streaming decode.
pub struct TelemetryTracker {
    start_time: Instant,
    prefill_end: Option<Instant>,
    prompt_tokens: usize,
    generated_tokens: usize,
    starting_vram: u64,
    peak_vram: u64,
}

impl TelemetryTracker {
    /// Start tracking with the known prompt token count.
    pub fn start(prompt_tokens: usize) -> Self {
        let current_vram = Self::query_vram();
        Self {
            start_time: Instant::now(),
            prefill_end: None,
            prompt_tokens,
            generated_tokens: 0,
            starting_vram: current_vram,
            peak_vram: current_vram,
        }
    }

    /// Record when the first token arrives (captures TTFT).
    pub fn record_first_token(&mut self) {
        if self.prefill_end.is_none() {
            self.prefill_end = Some(Instant::now());
        }
        self.generated_tokens += 1;
        self.poll_vram();
    }

    /// Record each subsequent token during decode.
    pub fn record_token(&mut self) {
        self.generated_tokens += 1;
        // Poll VRAM every 10 tokens to avoid overhead
        if self.generated_tokens % 10 == 0 {
            self.poll_vram();
        }
    }

    /// Finalize and produce the metrics report.
    pub fn finalize(self) -> ClusterMetrics {
        let end_time = Instant::now();
        let prefill_end = self.prefill_end.unwrap_or(end_time);

        ClusterMetrics {
            prompt_tokens: self.prompt_tokens,
            generated_tokens: self.generated_tokens,
            prefill_duration: prefill_end.duration_since(self.start_time),
            decode_duration: end_time.duration_since(prefill_end),
            starting_vram_used_bytes: self.starting_vram,
            peak_vram_used_bytes: self.peak_vram,
        }
    }

    /// Poll current VRAM and update peak if higher.
    fn poll_vram(&mut self) {
        let current = Self::query_vram();
        if current > self.peak_vram {
            self.peak_vram = current;
        }
    }

    /// Query GPU VRAM usage via NVML (returns 0 if feature disabled).
    fn query_vram() -> u64 {
        #[cfg(feature = "nvml")]
        {
            if let Ok(nvml) = Nvml::init() {
                if let Ok(device) = nvml.device_by_index(0) {
                    if let Ok(mem_info) = device.memory_info() {
                        return mem_info.used;
                    }
                }
            }
        }
        0
    }
}
