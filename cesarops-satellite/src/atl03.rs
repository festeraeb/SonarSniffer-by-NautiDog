//! ICESat-2 ATL03 photon-cloud reader (via the `hdf5` crate, requires libhdf5).
//!
//! Requires the `hdf5` feature: `cargo build --features hdf5`
//! Uses `read_raw` (returns Vec) to avoid ndarray version conflicts between
//! hdf5 0.8 (bundles ndarray 0.15) and our crate (ndarray 0.16).

use crate::types::BBox;

/// A filtered photon track segment within a bbox.
#[derive(Debug, Clone)]
pub struct PhotonSlice {
    pub heights: Vec<f32>,
    pub lats: Vec<f64>,
    pub lons: Vec<f64>,
    pub n_total_photons: usize,
    pub n_kept: usize,
    pub track: String,
}

pub const TRACKS: &[&str] = &["gt1l", "gt1r", "gt2l", "gt2r", "gt3l", "gt3r"];

#[cfg(feature = "hdf5")]
pub fn read_atl03_bbox(path: &std::path::Path, bbox: &BBox) -> anyhow::Result<Vec<PhotonSlice>> {
    let file = hdf5::File::open(path).map_err(|e| anyhow::anyhow!("HDF5 open: {e}"))?;
    let mut slices = Vec::new();
    for &track in TRACKS {
        match read_track(&file, track, bbox) {
            Ok(Some(ps)) if ps.n_kept > 0 => slices.push(ps),
            Ok(_) => {}
            Err(e) => tracing::debug!("ATL03 track {track} skipped: {e}"),
        }
    }
    Ok(slices)
}

#[cfg(feature = "hdf5")]
fn read_raw_f64(file: &hdf5::File, path: &str) -> anyhow::Result<Vec<f64>> {
    let ds = file.dataset(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    ds.read_raw::<f64>().map_err(|e| anyhow::anyhow!("{path} read: {e}"))
}

#[cfg(feature = "hdf5")]
fn read_raw_f32(file: &hdf5::File, path: &str) -> anyhow::Result<Vec<f32>> {
    let ds = file.dataset(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    ds.read_raw::<f32>().map_err(|e| anyhow::anyhow!("{path} read: {e}"))
}

#[cfg(feature = "hdf5")]
fn read_raw_u8(file: &hdf5::File, path: &str) -> anyhow::Result<(Vec<u8>, Vec<usize>)> {
    let ds = file.dataset(path).map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let shape = ds.shape();
    let data = ds.read_raw::<u8>().map_err(|e| anyhow::anyhow!("{path} read: {e}"))?;
    Ok((data, shape))
}

#[cfg(feature = "hdf5")]
fn read_track(file: &hdf5::File, track: &str, bbox: &BBox) -> anyhow::Result<Option<PhotonSlice>> {
    // Coarse check: segment-level geolocation.
    let seg_lat = read_raw_f64(file, &format!("{track}/geolocation/reference_photon_lat"))?;
    let seg_lon = read_raw_f64(file, &format!("{track}/geolocation/reference_photon_lon"))?;

    let any_in_bbox = seg_lat.iter().zip(seg_lon.iter()).any(|(&la, &lo)| {
        la >= bbox.lat_min && la <= bbox.lat_max && lo >= bbox.lon_min && lo <= bbox.lon_max
    });
    if !any_in_bbox {
        return Ok(None);
    }

    // Photon-level arrays.
    let h_ph = read_raw_f32(file, &format!("{track}/heights/h_ph"))?;
    let n_total = h_ph.len();

    let (conf_flat, conf_shape) = read_raw_u8(file, &format!("{track}/heights/signal_conf_ph"))?;
    let n_cols = if conf_shape.len() == 2 { conf_shape[1] } else { 1 };

    let lat_ph = read_raw_f64(file, &format!("{track}/heights/lat_ph"))?;
    let lon_ph = read_raw_f64(file, &format!("{track}/heights/lon_ph"))?;

    // Filter: bbox + inland-water confidence >= 2 + valid height.
    let iw_col = 4.min(n_cols.saturating_sub(1));
    let mut heights = Vec::new();
    let mut lats = Vec::new();
    let mut lons = Vec::new();

    for i in 0..n_total {
        let h = h_ph[i];
        if !h.is_finite() || h >= 3.4e38 {
            continue;
        }
        let la = *lat_ph.get(i).unwrap_or(&0.0);
        let lo = *lon_ph.get(i).unwrap_or(&0.0);
        if la < bbox.lat_min || la > bbox.lat_max || lo < bbox.lon_min || lo > bbox.lon_max {
            continue;
        }
        let conf = if n_cols > 1 {
            conf_flat.get(i * n_cols + iw_col).copied().unwrap_or(0)
        } else {
            conf_flat.get(i).copied().unwrap_or(0)
        };
        if conf < 2 {
            continue;
        }
        heights.push(h);
        lats.push(la);
        lons.push(lo);
    }

    Ok(Some(PhotonSlice {
        n_total_photons: n_total,
        n_kept: heights.len(),
        heights,
        lats,
        lons,
        track: track.to_string(),
    }))
}

#[cfg(not(feature = "hdf5"))]
pub fn read_atl03_bbox(_path: &std::path::Path, _bbox: &BBox) -> anyhow::Result<Vec<PhotonSlice>> {
    anyhow::bail!("ATL03 reader requires --features hdf5 (libhdf5-dev on this host)")
}
