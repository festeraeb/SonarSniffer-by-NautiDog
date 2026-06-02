//! Spectral indices and signal analysis.
//!
//! Ports:
//!   `_chip_ratio_ndwi`, `_zscore_series` from temporal_stack_engine.py
//!   `compare_grids`, `_radial_psd`, `_band_split_indices` from wh2k_ab_attenuation.py

use anyhow::Result;
use ndarray::{Array1, Array2, ArrayView2};
use nauticuvs::curvelet_forward;
use rustfft::{num_complex::Complex, FftPlanner};

// ── Spectral indices ──────────────────────────────────────────────────────────

/// NDWI = (Green − NIR) / (Green + NIR)
/// Green = B03, NIR = B08 for Sentinel-2.
pub fn ndwi(green: ArrayView2<f32>, nir: ArrayView2<f32>) -> Array2<f32> {
    let g = green.mapv(|v| v as f64);
    let n = nir.mapv(|v| v as f64);
    let denom = &g + &n;
    let r = (&g - &n) / denom.mapv(|d| if d.abs() < 1e-9 { 1e-9 } else { d });
    r.mapv(|v| if v.is_finite() { v as f32 } else { f32::NAN })
}

/// NDVI = (NIR − Red) / (NIR + Red)
/// NIR = B08, Red = B04 for Sentinel-2.
pub fn ndvi(nir: ArrayView2<f32>, red: ArrayView2<f32>) -> Array2<f32> {
    let n = nir.mapv(|v| v as f64);
    let r = red.mapv(|v| v as f64);
    let denom = &n + &r;
    let result = (&n - &r) / denom.mapv(|d| if d.abs() < 1e-9 { 1e-9 } else { d });
    result.mapv(|v| if v.is_finite() { v as f32 } else { f32::NAN })
}

/// NDTI (Normalised Difference Turbidity Index) = (Red − Green) / (Red + Green)
/// Red = B04, Green = B03.  Proxy for sediment/turbidity.
pub fn ndti(red: ArrayView2<f32>, green: ArrayView2<f32>) -> Array2<f32> {
    let r = red.mapv(|v| v as f64);
    let g = green.mapv(|v| v as f64);
    let denom = &r + &g;
    let result = (&r - &g) / denom.mapv(|d| if d.abs() < 1e-9 { 1e-9 } else { d });
    result.mapv(|v| if v.is_finite() { v as f32 } else { f32::NAN })
}

/// SPM (Suspended Particulate Matter) proxy: ratio of B04/B03.
pub fn spm_index(red: ArrayView2<f32>, green: ArrayView2<f32>) -> Array2<f32> {
    let r = red.mapv(|v| v as f64);
    let g = green.mapv(|v| v as f64);
    (r / g.mapv(|v| if v.abs() < 1e-9 { 1e-9 } else { v }))
        .mapv(|v| if v.is_finite() { v as f32 } else { f32::NAN })
}

// ── Z-score helpers ────────────────────────────────────────────────────────────

/// Compute z-score of the last element in a finite-value series.
/// Returns `None` if fewer than 3 finite values are present.
///
/// Mirrors `_zscore_series()` in temporal_stack_engine.py.
pub fn zscore_last(series: &[f64]) -> Option<f64> {
    let finite: Vec<f64> = series.iter().copied().filter(|v| v.is_finite()).collect();
    if finite.len() < 3 {
        return None;
    }
    let n = finite.len() as f64;
    let mu = finite.iter().sum::<f64>() / n;
    let var = finite.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / n;
    let sigma = var.sqrt();
    if sigma < 1e-9 {
        return Some(0.0);
    }
    Some((*finite.last().unwrap() - mu) / sigma)
}

/// Mean of a flat array slice, ignoring NaN.
pub fn nanmean(vals: &[f32]) -> Option<f64> {
    let finite: Vec<f64> = vals.iter().filter(|v| v.is_finite()).map(|v| *v as f64).collect();
    if finite.is_empty() {
        return None;
    }
    Some(finite.iter().sum::<f64>() / finite.len() as f64)
}

/// Std-dev of a flat array slice, ignoring NaN.
pub fn nanstd(vals: &[f32]) -> Option<f64> {
    let finite: Vec<f64> = vals.iter().filter(|v| v.is_finite()).map(|v| *v as f64).collect();
    if finite.len() < 2 {
        return None;
    }
    let mu = finite.iter().sum::<f64>() / finite.len() as f64;
    let var = finite.iter().map(|v| (v - mu).powi(2)).sum::<f64>() / finite.len() as f64;
    Some(var.sqrt())
}

/// Curvelet AC/coarse energy ratio using local full-precision nauticuvs.
///
/// Returns `None` when transform fails or grid is too small.
pub fn curvelet_energy_ratio(grid: ArrayView2<f32>, num_scales: usize) -> Option<f64> {
    let (h, w) = grid.dim();
    if h < 8 || w < 8 {
        return None;
    }
    let coeffs = curvelet_forward(&grid.to_owned(), num_scales).ok()?;

    let mut ac = 0.0_f64;
    for scale in &coeffs.detail {
        for sb in scale {
            ac += sb.iter().map(|c| c.norm_sqr()).sum::<f64>();
        }
    }
    ac += coeffs.fine.iter().map(|c| c.norm_sqr()).sum::<f64>();
    let coarse: f64 = coeffs.coarse.iter().map(|c| c.norm_sqr()).sum();

    Some(if coarse > 1e-12 {
        ac / coarse
    } else {
        ac / ((h * w) as f64 + 1.0)
    })
}

// ── Radial Power Spectral Density ─────────────────────────────────────────────
//
// Ports `_radial_psd`, `_band_split_indices`, `compare_grids` from wh2k_ab_attenuation.py.

pub struct RadialSpectrum {
    pub frequencies: Vec<f64>, // cycles per metre
    pub power: Vec<f64>,
}

/// Compute a radial-average power spectral density over a 2-D grid.
///
/// `cell_size_m` is the spatial resolution of the grid (metres per pixel).
/// `n_bins` is the number of radial frequency bins.
pub fn radial_psd(grid: ArrayView2<f64>, cell_size_m: f64, n_bins: usize) -> RadialSpectrum {
    let (h, w) = grid.dim();
    // Apply 2-D Hanning window
    let win: Array2<f64> = {
        let wy: Array1<f64> = (0..h)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (h as f64 - 1.0)).cos()))
            .collect::<Vec<_>>()
            .into();
        let wx: Array1<f64> = (0..w)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f64::consts::PI * i as f64 / (w as f64 - 1.0)).cos()))
            .collect::<Vec<_>>()
            .into();
        let mut win = Array2::zeros((h, w));
        for r in 0..h {
            for c in 0..w {
                win[[r, c]] = wy[r] * wx[c];
            }
        }
        win
    };
    let mu = grid.mean().unwrap_or(0.0);
    let signal: Vec<f64> = (&grid - mu).iter().zip(win.iter()).map(|(g, w)| g * w).collect();

    // 2D FFT using rustfft (row-by-row then column-by-column)
    let mut planner = FftPlanner::<f64>::new();
    let fft_row = planner.plan_fft_forward(w);
    let fft_col = planner.plan_fft_forward(h);

    let mut complex: Vec<Vec<Complex<f64>>> = signal
        .chunks(w)
        .map(|row| {
            let mut v: Vec<Complex<f64>> = row.iter().map(|&x| Complex::new(x, 0.0)).collect();
            fft_row.process(&mut v);
            v
        })
        .collect();

    // Column FFTs
    for c in 0..w {
        let mut col: Vec<Complex<f64>> = (0..h).map(|r| complex[r][c]).collect();
        fft_col.process(&mut col);
        for r in 0..h {
            complex[r][c] = col[r];
        }
    }

    // Power
    let power_flat: Vec<f64> = (0..h)
        .flat_map(|r| (0..w).map(move |c| (r, c)))
        .map(|(r, c)| complex[r][c].norm_sqr())
        .collect();

    // Radial frequencies
    let ky: Vec<f64> = (0..h)
        .map(|i| {
            let f = i as f64 / (h as f64 * cell_size_m);
            if i < h / 2 { f } else { f - 1.0 / cell_size_m }
        })
        .collect();
    let kx: Vec<f64> = (0..w)
        .map(|i| {
            let f = i as f64 / (w as f64 * cell_size_m);
            if i < w / 2 { f } else { f - 1.0 / cell_size_m }
        })
        .collect();

    let k_flat: Vec<f64> = (0..h)
        .flat_map(|r| (0..w).map(move |c| (r, c)))
        .map(|(r, c)| (ky[r] * ky[r] + kx[c] * kx[c]).sqrt())
        .collect();

    let k_max = k_flat.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    if k_max <= 0.0 {
        return RadialSpectrum { frequencies: vec![], power: vec![] };
    }

    let edges: Vec<f64> = (0..=n_bins).map(|i| i as f64 * k_max / n_bins as f64).collect();
    let mut k_centers = Vec::with_capacity(n_bins);
    let mut p_means = Vec::with_capacity(n_bins);

    for i in 0..n_bins {
        let mask: Vec<f64> = k_flat
            .iter()
            .zip(power_flat.iter())
            .filter(|(&k, _)| k >= edges[i] && k < edges[i + 1])
            .map(|(_, &p)| p)
            .collect();
        if mask.is_empty() {
            continue;
        }
        k_centers.push((edges[i] + edges[i + 1]) / 2.0);
        p_means.push(mask.iter().sum::<f64>() / mask.len() as f64);
    }

    RadialSpectrum { frequencies: k_centers, power: p_means }
}

/// Split a frequency array into low-band (≤ 25th percentile) and high-band (≥ 75th percentile)
/// index sets.
///
/// Mirrors `_band_split_indices()` from wh2k_ab_attenuation.py.
pub fn band_split_indices(freqs: &[f64]) -> (Vec<usize>, Vec<usize>) {
    let mut sorted = freqs.to_vec();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let n = sorted.len();
    let q1 = sorted[n / 4];
    let q3 = sorted[(3 * n) / 4];

    let low: Vec<usize> = freqs.iter().enumerate().filter(|(_, &f)| f <= q1).map(|(i, _)| i).collect();
    let high: Vec<usize> = freqs.iter().enumerate().filter(|(_, &f)| f >= q3).map(|(i, _)| i).collect();
    (low, high)
}

/// A/B attenuation summary — high-frequency energy loss from grid A to grid B.
#[derive(Debug, Clone)]
pub struct AttenuationSummary {
    pub high_to_low_ratio_a: f64,
    pub high_to_low_ratio_b: f64,
    pub attenuation_factor: f64,
    pub attenuation_db: f64,
    pub corr_spectral: f64,
}

/// Compare two grids' PSD and return attenuation metrics.
///
/// Both grids must have the same shape (will be min-cropped if different).
pub fn compare_grids_attenuation(
    a: &Array2<f64>,
    b: &Array2<f64>,
    cell_size_m: f64,
) -> Result<AttenuationSummary> {
    let (h, w) = (a.dim().0.min(b.dim().0), a.dim().1.min(b.dim().1));
    let a = a.slice(ndarray::s![..h, ..w]);
    let b = b.slice(ndarray::s![..h, ..w]);

    let spec_a = radial_psd(a, cell_size_m, 80);
    let spec_b = radial_psd(b.view(), cell_size_m, 80);

    let n = spec_a.frequencies.len().min(spec_b.frequencies.len());
    if n < 10 {
        anyhow::bail!("Not enough spectral bins for A/B comparison");
    }

    let freqs = &spec_a.frequencies[..n];
    let (low_idx, high_idx) = band_split_indices(freqs);

    let mean = |arr: &[f64], idx: &[usize]| -> f64 {
        if idx.is_empty() {
            return 0.0;
        }
        idx.iter().map(|&i| arr[i]).sum::<f64>() / idx.len() as f64
    };

    let low_a = mean(&spec_a.power[..n], &low_idx);
    let low_b = mean(&spec_b.power[..n], &low_idx);
    let high_a = mean(&spec_a.power[..n], &high_idx);
    let high_b = mean(&spec_b.power[..n], &high_idx);

    let h2l_a = if low_a > 0.0 { high_a / low_a } else { 0.0 };
    let h2l_b = if low_b > 0.0 { high_b / low_b } else { 0.0 };
    let attenuation_factor = if h2l_a > 0.0 { h2l_b / h2l_a } else { 0.0 };
    let attenuation_db = 10.0 * attenuation_factor.max(1e-12_f64).log10();

    // Spectral correlation: log10(power_a) vs log10(power_b)
    let pa_log: Vec<f64> = spec_a.power[..n].iter().map(|&p| p.max(1e-12).log10()).collect();
    let pb_log: Vec<f64> = spec_b.power[..n].iter().map(|&p| p.max(1e-12).log10()).collect();
    let corr_spectral = pearson_r(&pa_log, &pb_log);

    Ok(AttenuationSummary { high_to_low_ratio_a: h2l_a, high_to_low_ratio_b: h2l_b, attenuation_factor, attenuation_db, corr_spectral })
}

fn pearson_r(a: &[f64], b: &[f64]) -> f64 {
    let n = a.len().min(b.len()) as f64;
    if n < 2.0 {
        return 0.0;
    }
    let mu_a = a.iter().sum::<f64>() / n;
    let mu_b = b.iter().sum::<f64>() / n;
    let num: f64 = a.iter().zip(b.iter()).map(|(x, y)| (x - mu_a) * (y - mu_b)).sum();
    let da: f64 = a.iter().map(|x| (x - mu_a).powi(2)).sum::<f64>().sqrt();
    let db: f64 = b.iter().map(|y| (y - mu_b).powi(2)).sum::<f64>().sqrt();
    if da < 1e-12 || db < 1e-12 { 0.0 } else { num / (da * db) }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;
    use ndarray::array;

    #[test]
    fn ndwi_pure_water() {
        // Green >> NIR → positive NDWI (water)
        let green = array![[0.3_f32, 0.3], [0.3, 0.3]];
        let nir = array![[0.05_f32, 0.05], [0.05, 0.05]];
        let r = ndwi(green.view(), nir.view());
        assert!(r[[0, 0]] > 0.5, "water should have strong positive NDWI");
    }

    #[test]
    fn zscore_last_constant() {
        let s = vec![1.0, 1.0, 1.0, 1.0, 1.0];
        assert_eq!(zscore_last(&s), Some(0.0));
    }

    #[test]
    fn zscore_last_outlier() {
        let s = vec![0.0, 0.0, 0.0, 0.0, 10.0];
        let z = zscore_last(&s).unwrap();
        assert!(z > 1.5, "last point is an outlier");
    }

    #[test]
    fn ndti_turbid_vs_clear() {
        let red_turbid = array![[0.15_f32]];
        let green_turbid = array![[0.08_f32]];
        let turbid = ndti(red_turbid.view(), green_turbid.view())[[0, 0]];
        let red_clear = array![[0.04_f32]];
        let green_clear = array![[0.10_f32]];
        let clear = ndti(red_clear.view(), green_clear.view())[[0, 0]];
        assert!(turbid > clear, "turbid should have higher NDTI");
    }
}
