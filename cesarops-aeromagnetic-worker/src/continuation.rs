//! Upward / downward continuation + satellite proof.
//!
//! Ports `pipelines/mag/wh2k_upward_continuation.py`:
//!   - `upward_continue`   — Fourier-domain `F(k)·exp(-2π|k|·Δz)`
//!   - `downward_continue` — inverse, `exp(+2π|k|·Δz)` capped at `max_amplification`
//!   - `run_satellite_proof` — simulate a wreck's signal at satellite altitude and
//!     compare against real satellite data, tagging SAT_VISIBLE.
//!
//! The Python implementation uses `numpy.fft.fft2`; here we use `rustfft` to do a
//! 2D FFT (row-wise then column-wise 1D transforms). `rustfft` is unnormalised,
//! so the inverse pass divides by `nx*ny` to match NumPy's normalisation.

use rustfft::num_complex::Complex;
use rustfft::FftPlanner;
use serde::Serialize;

// ── Tunable knobs (wh2k_upward_continuation.py defaults) ────────────────────

/// Default satellite continuation height (m). Ports `continuation_height_m=400_000`.
pub const DEFAULT_CONTINUATION_HEIGHT_M: f64 = 400_000.0;
/// Default minimum anomaly to count as "visible" (nT). Ports `threshold_nt=0.5`.
pub const DEFAULT_THRESHOLD_NT: f64 = 0.5;
/// Default cap on downward-continuation amplification. Ports `max_amplification=100`.
pub const DEFAULT_MAX_AMPLIFICATION: f64 = 100.0;

// ── FFT helpers ──────────────────────────────────────────────────────────────

/// NumPy-compatible discrete FFT sample frequencies.
///
/// Mirrors `numpy.fft.fftfreq(n, d)`:
///   f = [0, 1, …, (n-1)/2, -(n/2), …, -1] / (n·d)
fn fftfreq(n: usize, d: f64) -> Vec<f64> {
    let val = 1.0 / (n as f64 * d);
    let mut f = vec![0.0; n];
    let half = (n - 1) / 2; // last non-negative index
    for (i, fi) in f.iter_mut().enumerate() {
        let k = if i <= half {
            i as i64
        } else {
            i as i64 - n as i64
        };
        *fi = k as f64 * val;
    }
    f
}

/// In-place 2D FFT (forward or inverse) on a row-major `ny × nx` buffer.
fn fft2_inplace(data: &mut [Complex<f64>], ny: usize, nx: usize, inverse: bool) {
    let mut planner = FftPlanner::<f64>::new();
    // Row transforms (length nx).
    let fft_row = if inverse {
        planner.plan_fft_inverse(nx)
    } else {
        planner.plan_fft_forward(nx)
    };
    for r in 0..ny {
        fft_row.process(&mut data[r * nx..(r + 1) * nx]);
    }
    // Column transforms (length ny).
    let fft_col = if inverse {
        planner.plan_fft_inverse(ny)
    } else {
        planner.plan_fft_forward(ny)
    };
    let mut col = vec![Complex::new(0.0, 0.0); ny];
    for c in 0..nx {
        for r in 0..ny {
            col[r] = data[r * nx + c];
        }
        fft_col.process(&mut col);
        for r in 0..ny {
            data[r * nx + c] = col[r];
        }
    }
}

/// Apply a Fourier-domain continuation operator with sign `s` (negative for
/// upward, positive for downward) and an optional amplification cap.
fn continue_field(
    grid: &[f64],
    ny: usize,
    nx: usize,
    height_m: f64,
    cell_size_m: f64,
    sign: f64,
    max_amplification: Option<f64>,
) -> Vec<f64> {
    assert_eq!(grid.len(), ny * nx, "grid length must equal ny*nx");

    // Forward FFT.
    let mut spec: Vec<Complex<f64>> = grid.iter().map(|&v| Complex::new(v, 0.0)).collect();
    fft2_inplace(&mut spec, ny, nx, false);

    let ky = fftfreq(ny, cell_size_m);
    let kx = fftfreq(nx, cell_size_m);

    for r in 0..ny {
        for c in 0..nx {
            let k_mag = (kx[c] * kx[c] + ky[r] * ky[r]).sqrt();
            let mut filt = (sign * 2.0 * std::f64::consts::PI * k_mag * height_m).exp();
            if let Some(cap) = max_amplification {
                filt = filt.min(cap);
            }
            spec[r * nx + c] *= filt;
        }
    }

    // Inverse FFT + NumPy normalisation (divide by N).
    fft2_inplace(&mut spec, ny, nx, true);
    let norm = (nx * ny) as f64;
    spec.iter().map(|z| z.re / norm).collect()
}

/// Upward-continue a 2D potential-field grid to `continuation_height_m`.
///
/// Ports `upward_continue`: `F_continued(k) = F(k)·exp(-2π|k|·Δz)`. Attenuates
/// high-frequency (shallow/small) sources while preserving low-frequency ones.
/// `grid` is row-major `ny × nx`. Returns a new grid of the same shape (nT).
pub fn upward_continue(
    grid: &[f64],
    ny: usize,
    nx: usize,
    continuation_height_m: f64,
    cell_size_m: f64,
) -> Vec<f64> {
    continue_field(grid, ny, nx, continuation_height_m, cell_size_m, -1.0, None)
}

/// Downward-continue a 2D potential-field grid (inverse of upward).
///
/// Ports `downward_continue`: `exp(+2π|k|·Δz)` capped at `max_amplification` to
/// prevent numerical explosion (downward continuation amplifies noise).
pub fn downward_continue(
    grid: &[f64],
    ny: usize,
    nx: usize,
    continuation_depth_m: f64,
    cell_size_m: f64,
    max_amplification: f64,
) -> Vec<f64> {
    continue_field(
        grid,
        ny,
        nx,
        continuation_depth_m,
        cell_size_m,
        1.0,
        Some(max_amplification),
    )
}

// ── Satellite proof ──────────────────────────────────────────────────────────

/// Result of comparing a simulated satellite signal to real satellite data.
/// Ports the `SatelliteProofResult` dataclass.
#[derive(Debug, Clone, Serialize)]
pub struct SatelliteProofResult {
    pub wreck_name: String,
    pub lat: f64,
    pub lon: f64,
    pub aero_peak_nt: f64,
    pub simulated_sat_peak_nt: f64,
    pub real_sat_value_nt: f64,
    pub real_sat_background_nt: f64,
    pub real_sat_anomaly_nt: f64,
    pub sim_predicts_visible: bool,
    pub real_shows_bump: bool,
    pub correlation_confirmed: bool,
    pub sat_detection_viable: bool,
    /// SAT_VISIBLE tag: simulated continuation peak clears the threshold.
    pub sat_visible: bool,
    pub continuation_height_m: f64,
    pub threshold_nt: f64,
}

fn window_peak_abs(
    grid: &[f64],
    ny: usize,
    nx: usize,
    r: usize,
    c: usize,
    win: isize,
) -> f64 {
    let mut peak = 0.0f64;
    let r0 = (r as isize - win).max(0) as usize;
    let r1 = ((r as isize + win + 1).min(ny as isize)) as usize;
    let c0 = (c as isize - win).max(0) as usize;
    let c1 = ((c as isize + win + 1).min(nx as isize)) as usize;
    for rr in r0..r1 {
        for cc in c0..c1 {
            peak = peak.max(grid[rr * nx + cc].abs());
        }
    }
    peak
}

fn median(values: &mut [f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        (values[n / 2 - 1] + values[n / 2]) / 2.0
    }
}

/// Run the satellite proof test for a single known wreck.
///
/// Ports `run_satellite_proof`:
///   1. Aero peak at the wreck location (5-px window).
///   2. Upward-continue the whole aero grid to satellite altitude.
///   3. Read the simulated peak at the wreck.
///   4. Read the real satellite value + annulus-median background.
///   5. Verdicts: SAT_VISIBLE when the simulated peak clears `threshold_nt`;
///      viable when both the simulation and the real data agree.
#[allow(clippy::too_many_arguments)]
pub fn run_satellite_proof(
    aero_grid: &[f64],
    aero_ny: usize,
    aero_nx: usize,
    aero_cell_size_m: f64,
    satellite_grid: &[f64],
    sat_ny: usize,
    sat_nx: usize,
    target_aero: (usize, usize),
    target_sat: (usize, usize),
    wreck_name: &str,
    lat: f64,
    lon: f64,
    continuation_height_m: f64,
    threshold_nt: f64,
    annulus_radius_px: isize,
) -> SatelliteProofResult {
    let (r_aero, c_aero) = target_aero;
    let (r_sat, c_sat) = target_sat;
    const WIN: isize = 5;

    // 1. Aero peak.
    let aero_peak = window_peak_abs(aero_grid, aero_ny, aero_nx, r_aero, c_aero, WIN);

    // 2. Upward continue + 3. simulated peak.
    let continued = upward_continue(
        aero_grid,
        aero_ny,
        aero_nx,
        continuation_height_m,
        aero_cell_size_m,
    );
    let sim_sat_peak = window_peak_abs(&continued, aero_ny, aero_nx, r_aero, c_aero, WIN);

    // 4. Real satellite value at the (clipped) target.
    let rr = r_sat.min(sat_ny.saturating_sub(1));
    let cc = c_sat.min(sat_nx.saturating_sub(1));
    let real_sat_value = satellite_grid[rr * sat_nx + cc];

    // Background = median over an annulus around the target.
    let mut annulus: Vec<f64> = Vec::new();
    for r in 0..sat_ny {
        for c in 0..sat_nx {
            let dr = r as f64 - r_sat as f64;
            let dc = c as f64 - c_sat as f64;
            let dist = (dr * dr + dc * dc).sqrt();
            let inner = annulus_radius_px as f64;
            let outer = (annulus_radius_px * 3) as f64;
            if dist > inner && dist < outer {
                let v = satellite_grid[r * sat_nx + c];
                if !v.is_nan() {
                    annulus.push(v);
                }
            }
        }
    }
    let background = median(&mut annulus);
    let real_anomaly = real_sat_value - background;

    // 5. Verdicts.
    let sim_predicts = sim_sat_peak >= threshold_nt;
    let real_bump = real_anomaly.abs() >= threshold_nt;
    let correlation = sim_predicts && real_bump;

    SatelliteProofResult {
        wreck_name: wreck_name.to_string(),
        lat,
        lon,
        aero_peak_nt: aero_peak,
        simulated_sat_peak_nt: sim_sat_peak,
        real_sat_value_nt: real_sat_value,
        real_sat_background_nt: background,
        real_sat_anomaly_nt: real_anomaly,
        sim_predicts_visible: sim_predicts,
        real_shows_bump: real_bump,
        correlation_confirmed: correlation,
        sat_detection_viable: correlation,
        sat_visible: sim_predicts,
        continuation_height_m,
        threshold_nt,
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn std_dev(v: &[f64]) -> f64 {
        let n = v.len() as f64;
        let mean = v.iter().sum::<f64>() / n;
        (v.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt()
    }

    fn mean(v: &[f64]) -> f64 {
        v.iter().sum::<f64>() / v.len() as f64
    }

    /// fftfreq must match NumPy for even and odd lengths.
    #[test]
    fn test_fftfreq_matches_numpy() {
        // np.fft.fftfreq(4, 1.0) = [0, 0.25, -0.5, -0.25]
        let f = fftfreq(4, 1.0);
        let expect = [0.0, 0.25, -0.5, -0.25];
        for (a, b) in f.iter().zip(expect) {
            assert!((a - b).abs() < 1e-12, "even fftfreq mismatch: {a} vs {b}");
        }
        // np.fft.fftfreq(5, 1.0) = [0, 0.2, 0.4, -0.4, -0.2]
        let f = fftfreq(5, 1.0);
        let expect = [0.0, 0.2, 0.4, -0.4, -0.2];
        for (a, b) in f.iter().zip(expect) {
            assert!((a - b).abs() < 1e-12, "odd fftfreq mismatch: {a} vs {b}");
        }
    }

    /// Round-trip: zero continuation height leaves the grid unchanged (the
    /// operator is exp(0)=1, and FFT→IFFT is the identity after normalisation).
    #[test]
    fn test_zero_height_is_identity() {
        let ny = 8;
        let nx = 8;
        let grid: Vec<f64> = (0..ny * nx).map(|i| (i as f64 * 1.3).sin() * 10.0 + 50.0).collect();
        let out = upward_continue(&grid, ny, nx, 0.0, 100.0);
        for (a, b) in grid.iter().zip(&out) {
            assert!((a - b).abs() < 1e-9, "zero-height continuation must be identity");
        }
    }

    /// Upward continuation must attenuate a high-frequency ripple while
    /// preserving the DC (mean) level. Ports the physical intent of
    /// `upward_continue` in wh2k_upward_continuation.py.
    #[test]
    fn test_upward_continue_attenuates_high_freq() {
        let ny = 32;
        let nx = 32;
        let cell = 100.0;
        let base = 100.0;
        // Highest-frequency ripple: alternates sign every column.
        let mut grid = vec![0.0f64; ny * nx];
        for r in 0..ny {
            for c in 0..nx {
                let ripple = if c % 2 == 0 { 10.0 } else { -10.0 };
                grid[r * nx + c] = base + ripple;
            }
        }

        let std_before = std_dev(&grid);
        let mean_before = mean(&grid);

        // Modest height so the operator attenuates strongly but stays finite.
        let out = upward_continue(&grid, ny, nx, 200.0, cell);

        let std_after = std_dev(&out);
        let mean_after = mean(&out);

        assert!(
            std_after < std_before * 0.5,
            "high-freq ripple must be attenuated: before={std_before:.3} after={std_after:.3}"
        );
        assert!(
            (mean_after - mean_before).abs() < 1e-6,
            "DC level must be preserved: before={mean_before:.6} after={mean_after:.6}"
        );
    }

    /// Lower-frequency content is attenuated less than higher-frequency content.
    #[test]
    fn test_upward_continue_low_freq_survives_more() {
        let ny = 32;
        let nx = 32;
        let cell = 100.0;
        let pi = std::f64::consts::PI;

        // Low-frequency: one full cosine cycle across the grid.
        let low: Vec<f64> = (0..ny * nx)
            .map(|i| {
                let c = (i % nx) as f64;
                10.0 * (2.0 * pi * c / nx as f64).cos()
            })
            .collect();
        // High-frequency: eight cycles across the grid.
        let high: Vec<f64> = (0..ny * nx)
            .map(|i| {
                let c = (i % nx) as f64;
                10.0 * (2.0 * pi * 8.0 * c / nx as f64).cos()
            })
            .collect();

        let h = 300.0;
        let low_out = upward_continue(&low, ny, nx, h, cell);
        let high_out = upward_continue(&high, ny, nx, h, cell);

        let low_ret = std_dev(&low_out) / std_dev(&low);
        let high_ret = std_dev(&high_out) / std_dev(&high);
        assert!(
            low_ret > high_ret,
            "low-freq must retain more energy than high-freq: low={low_ret:.4} high={high_ret:.4}"
        );
    }

    /// Downward continuation amplifies (relative to upward), but the cap bounds it.
    #[test]
    fn test_downward_continue_capped() {
        let ny = 16;
        let nx = 16;
        let cell = 100.0;
        let pi = std::f64::consts::PI;
        let grid: Vec<f64> = (0..ny * nx)
            .map(|i| {
                let c = (i % nx) as f64;
                5.0 * (2.0 * pi * 2.0 * c / nx as f64).cos()
            })
            .collect();

        let up = upward_continue(&grid, ny, nx, 100.0, cell);
        let down = downward_continue(&grid, ny, nx, 100.0, cell, DEFAULT_MAX_AMPLIFICATION);

        // Downward continuation enhances the ripple relative to upward.
        assert!(
            std_dev(&down) > std_dev(&up),
            "downward continuation must amplify relative to upward"
        );
        // Cap keeps it finite/bounded.
        assert!(down.iter().all(|v| v.is_finite()), "downward output must stay finite");
    }

    /// Satellite proof: a strong simulated peak above threshold with a real bump
    /// is tagged SAT_VISIBLE and viable; a flat real grid is not.
    #[test]
    fn test_satellite_proof_tagging() {
        let ny = 16;
        let nx = 16;
        // Aero grid with a strong central anomaly.
        let mut aero = vec![0.0f64; ny * nx];
        aero[8 * nx + 8] = 5000.0;
        aero[8 * nx + 7] = 4000.0;
        aero[7 * nx + 8] = 4000.0;

        // Real satellite grid: a clear bump at the target over a flat background.
        let mut sat = vec![0.0f64; ny * nx];
        sat[8 * nx + 8] = 2.0;

        // Very small continuation height so the simulated peak survives above 0.5 nT.
        let res = run_satellite_proof(
            &aero, ny, nx, 1000.0, &sat, ny, nx, (8, 8), (8, 8), "SS Test", 42.0, -80.0, 1.0, 0.5,
            3,
        );
        assert!(res.aero_peak_nt >= 5000.0, "aero peak must be captured");
        assert!(res.sat_visible, "strong continued peak should be SAT_VISIBLE");
        assert!(res.real_shows_bump, "real 2 nT bump exceeds 0.5 nT threshold");
        assert!(res.sat_detection_viable, "both agree → viable");

        // Flat real satellite grid → no bump → not viable even if sim predicts.
        let flat = vec![0.0f64; ny * nx];
        let res2 = run_satellite_proof(
            &aero, ny, nx, 1000.0, &flat, ny, nx, (8, 8), (8, 8), "SS Flat", 42.0, -80.0, 1.0, 0.5,
            3,
        );
        assert!(!res2.real_shows_bump, "flat grid shows no bump");
        assert!(!res2.sat_detection_viable, "no real bump → not viable");
    }
}
