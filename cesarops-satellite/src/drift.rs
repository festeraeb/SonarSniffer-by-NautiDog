//! Historical drift engine — haversine physics + storm-phase forward/backward drift.
//!
//! Ports `historical_drift.py`:
//!   `_haversine`, `_destination`, `_basic_drift`, `StormPhase`, `DriftConfig`,
//!   `forward_drift_run`, `backward_drift_run`, `analog_storm_score`
//!
//! Also ports buoy_analog.py analog storm matching logic.

use serde::{Deserialize, Serialize};

// ── Constants ─────────────────────────────────────────────────────────────────

const EARTH_R_KM: f64 = 6_371.0;
const KM_TO_NM: f64 = 0.539957;
const NM_TO_M: f64 = 1_852.0;
const EARTH_R_M: f64 = 6_371_000.0;
const DEFAULT_WINDAGE: f64 = 0.03;

// ── Geometry primitives ────────────────────────────────────────────────────────

/// Haversine distance in nautical miles.
pub fn haversine_nm(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let a = (dlat / 2.0).sin().powi(2)
        + lat1.to_radians().cos() * lat2.to_radians().cos() * (dlon / 2.0).sin().powi(2);
    EARTH_R_KM * 2.0 * a.sqrt().atan2((1.0 - a).sqrt()) * KM_TO_NM
}

/// Destination point from start + compass bearing + nautical miles.
pub fn destination(lat: f64, lon: f64, bearing_deg: f64, dist_nm: f64) -> (f64, f64) {
    let d = dist_nm * NM_TO_M / EARTH_R_M;
    let b = bearing_deg.to_radians();
    let lr = lat.to_radians();
    let lor = lon.to_radians();
    let lat2 = (lr.sin() * d.cos() + lr.cos() * d.sin() * b.cos()).asin();
    let lon2 = lor
        + f64::atan2(b.sin() * d.sin() * lr.cos(), d.cos() - lr.sin() * lat2.sin());
    (lat2.to_degrees(), lon2.to_degrees())
}

/// Compute drift velocity (u, v) in m/s given wind + current + wave.
///
/// Mirrors `_basic_drift()` in historical_drift.py.
/// windage fraction: 0.03 = 3% leeway (debris default).
pub fn basic_drift(
    wind_speed_ms: f64,
    wind_dir_deg: f64,
    current_u: f64,
    current_v: f64,
    _wave_ht: f64,
    windage: f64,
) -> (f64, f64) {
    let rad = wind_dir_deg.to_radians();
    let wu = wind_speed_ms * rad.sin();
    let wv = wind_speed_ms * rad.cos();
    (current_u + windage * wu, current_v + windage * wv)
}

// ── Storm phase ────────────────────────────────────────────────────────────────

/// A single constant-wind phase of a storm (mirrors Python StormPhase dataclass).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StormPhase {
    pub label: String,
    pub wind_speed_ms: f64,
    pub wind_dir_deg: f64,
    pub current_u: f64,
    pub current_v: f64,
    pub wave_ht_m: f64,
    pub dt_hours: f64,
}

/// A full storm event made up of sequential phases.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StormEvent {
    pub name: String,
    pub phases: Vec<StormPhase>,
}

// ── Drift particle ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftParticle {
    pub lat: f64,
    pub lon: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftStep {
    pub phase_label: String,
    pub dt_hours: f64,
    pub lat: f64,
    pub lon: f64,
    pub u_ms: f64,
    pub v_ms: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftRun {
    pub origin_lat: f64,
    pub origin_lon: f64,
    pub final_lat: f64,
    pub final_lon: f64,
    pub total_nm: f64,
    pub steps: Vec<DriftStep>,
}

// ── Drift config ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftConfig {
    pub windage: f64,
    /// Number of Monte-Carlo particles (spread estimation)
    pub n_particles: usize,
    /// Wind speed scatter σ (m/s) for ensemble
    pub wind_speed_sigma: f64,
    /// Wind direction scatter σ (degrees)
    pub wind_dir_sigma: f64,
}

impl Default for DriftConfig {
    fn default() -> Self {
        Self {
            windage: DEFAULT_WINDAGE,
            n_particles: 50,
            wind_speed_sigma: 1.5,
            wind_dir_sigma: 10.0,
        }
    }
}

// ── Forward drift ─────────────────────────────────────────────────────────────

/// Run a single deterministic forward drift trajectory.
///
/// `origin` — starting position (e.g. last-known position of vessel/object).
/// `storm`  — ordered storm phases to step through.
/// `cfg`    — windage + scatter parameters.
pub fn forward_drift_run(origin: DriftParticle, storm: &StormEvent, cfg: &DriftConfig) -> DriftRun {
    let mut lat = origin.lat;
    let mut lon = origin.lon;
    let mut steps = Vec::new();
    let origin_lat = lat;
    let origin_lon = lon;

    for phase in &storm.phases {
        let (u, v) = basic_drift(
            phase.wind_speed_ms,
            phase.wind_dir_deg,
            phase.current_u,
            phase.current_v,
            phase.wave_ht_m,
            cfg.windage,
        );
        // Convert velocity to displacement in nautical miles over dt_hours
        let speed_ms = (u * u + v * v).sqrt();
        let bearing_deg = f64::atan2(u, v).to_degrees().rem_euclid(360.0);
        let dist_nm = speed_ms * phase.dt_hours * 3600.0 / NM_TO_M;

        let (new_lat, new_lon) = destination(lat, lon, bearing_deg, dist_nm);
        steps.push(DriftStep {
            phase_label: phase.label.clone(),
            dt_hours: phase.dt_hours,
            lat: new_lat,
            lon: new_lon,
            u_ms: u,
            v_ms: v,
        });
        lat = new_lat;
        lon = new_lon;
    }

    let total_nm = haversine_nm(origin_lat, origin_lon, lat, lon);
    DriftRun { origin_lat, origin_lon, final_lat: lat, final_lon: lon, total_nm, steps }
}

/// Run a Monte-Carlo ensemble of forward drift runs (wind scatter).
pub fn forward_drift_ensemble(
    origin: DriftParticle,
    storm: &StormEvent,
    cfg: &DriftConfig,
) -> Vec<DriftRun> {
    use std::f64::consts::TAU;
    // LCG-based RNG (no external rand dep required)
    let mut seed: u64 = 0xDEADBEEF1909;
    let lcg = |s: &mut u64| -> f64 {
        *s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((*s >> 33) as f64) / (u32::MAX as f64)
    };

    (0..cfg.n_particles)
        .map(|_| {
            // Box-Muller normal samples
            let u1 = lcg(&mut seed).max(1e-9);
            let u2 = lcg(&mut seed);
            let z0 = (-2.0 * u1.ln()).sqrt() * (TAU * u2).cos();
            let z1 = (-2.0 * u1.ln()).sqrt() * (TAU * u2).sin();

            let mut scattered_storm = storm.clone();
            for phase in &mut scattered_storm.phases {
                phase.wind_speed_ms = (phase.wind_speed_ms + z0 * cfg.wind_speed_sigma).max(0.0);
                phase.wind_dir_deg = (phase.wind_dir_deg + z1 * cfg.wind_dir_sigma).rem_euclid(360.0);
            }
            forward_drift_run(origin.clone(), &scattered_storm, cfg)
        })
        .collect()
}

// ── Backward drift ────────────────────────────────────────────────────────────

/// Reverse-time drift from a recovery location back to estimate sinking origin.
/// Flips wind/current directions for each phase.
pub fn backward_drift_run(
    recovery: DriftParticle,
    storm: &StormEvent,
    cfg: &DriftConfig,
) -> DriftRun {
    let reversed_phases: Vec<StormPhase> = storm
        .phases
        .iter()
        .rev()
        .map(|p| StormPhase {
            wind_dir_deg: (p.wind_dir_deg + 180.0).rem_euclid(360.0),
            current_u: -p.current_u,
            current_v: -p.current_v,
            ..p.clone()
        })
        .collect();
    let reversed_storm = StormEvent {
        name: format!("{}_reversed", storm.name),
        phases: reversed_phases,
    };
    forward_drift_run(recovery, &reversed_storm, cfg)
}

// ── Analog storm scoring ───────────────────────────────────────────────────────

/// A modern storm analog candidate.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogStorm {
    pub year: i32,
    pub month: u32,
    pub day: u32,
    pub peak_wind_ms: f64,
    pub peak_dir_deg: f64,
    pub duration_hours: f64,
    /// Similarity score vs target event (0=identical, higher=worse)
    pub analog_score: f64,
}

/// Score a candidate modern storm against a target event's peak wind/dir/duration.
/// Lower score = better analog.
///
/// Mirrors `analog_storm_score()` in buoy_analog.py.
pub fn analog_storm_score(
    candidate_speed: f64,
    candidate_dir: f64,
    candidate_hours: f64,
    target_speed: f64,
    target_dir: f64,
    target_hours: f64,
) -> f64 {
    let speed_diff = (candidate_speed - target_speed).abs() / target_speed.max(1.0);
    let dir_diff_deg = {
        let d = (candidate_dir - target_dir).abs() % 360.0;
        if d > 180.0 { 360.0 - d } else { d }
    };
    let dir_diff = dir_diff_deg / 180.0; // normalise to [0, 1]
    let dur_diff = (candidate_hours - target_hours).abs() / target_hours.max(1.0);
    // Weighted sum: direction most important for drift, then speed, then duration
    0.5 * dir_diff + 0.35 * speed_diff + 0.15 * dur_diff
}

// ── Drift envelope ────────────────────────────────────────────────────────────

/// Compute the spread envelope (bounding box) of an ensemble of drift endpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftEnvelope {
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
    pub centroid_lat: f64,
    pub centroid_lon: f64,
    pub spread_nm: f64,
}

pub fn drift_envelope(runs: &[DriftRun]) -> Option<DriftEnvelope> {
    if runs.is_empty() {
        return None;
    }
    let lats: Vec<f64> = runs.iter().map(|r| r.final_lat).collect();
    let lons: Vec<f64> = runs.iter().map(|r| r.final_lon).collect();
    let lat_min = lats.iter().cloned().fold(f64::INFINITY, f64::min);
    let lat_max = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lon_min = lons.iter().cloned().fold(f64::INFINITY, f64::min);
    let lon_max = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let n = runs.len() as f64;
    let centroid_lat = lats.iter().sum::<f64>() / n;
    let centroid_lon = lons.iter().sum::<f64>() / n;
    // spread_nm = mean 1-σ radius around the centroid (matches Python
    // historical_drift.py `_run_particles`: spread = mean(haversine(centroid,
    // final)) over all particles).  Previously this used the bbox-diagonal/2,
    // which over-estimated the spread for elongated cones.
    let spread_nm = lats
        .iter()
        .zip(lons.iter())
        .map(|(&la, &lo)| haversine_nm(centroid_lat, centroid_lon, la, lo))
        .sum::<f64>()
        / n;
    Some(DriftEnvelope { lat_min, lat_max, lon_min, lon_max, centroid_lat, centroid_lon, spread_nm })
}

// ══════════════════════════════════════════════════════════════════════════════
// Faithful port of historical_drift.py Monte-Carlo physics
// ══════════════════════════════════════════════════════════════════════════════
//
// The structs/functions above keep the original Rust drift API stable.  The
// section below ports `historical_drift.py` more literally: per-storm-phase
// sub-stepping at DT_STEP=0.25h (15 min), per-step parameter scatter, and a
// `DriftResult` whose `spread_nm` is the mean 1-σ radius around the centroid.

/// 15-minute integration step (Python `DT_STEP = 0.25`).
pub const DT_STEP_HOURS: f64 = 0.25;
/// mph → m/s (Python `MPH_TO_MS`).
pub const MPH_TO_MS: f64 = 0.44704;
/// knots → m/s.
pub const KT_TO_MS: f64 = 0.514444;
/// feet → metres.
pub const FT_TO_M: f64 = 0.3048;

/// A storm phase in the Python `historical_drift.py` units (mph / kt / ft).
///
/// Mirrors the `StormPhase` dataclass there (distinct from [`StormPhase`] above,
/// which is the original Rust SI-unit phase kept for API stability).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhysPhase {
    pub label: String,
    pub wind_speed_mph: f64,
    /// meteorological FROM-direction, 0 = N.
    pub wind_dir_deg: f64,
    pub dt_hours: f64,
    pub wave_ht_ft: f64,
    pub current_speed_kt: f64,
    pub current_dir_deg: f64,
}

impl PhysPhase {
    pub fn new(
        label: &str,
        wind_speed_mph: f64,
        wind_dir_deg: f64,
        dt_hours: f64,
        wave_ht_ft: f64,
        current_speed_kt: f64,
        current_dir_deg: f64,
    ) -> Self {
        Self {
            label: label.into(),
            wind_speed_mph,
            wind_dir_deg,
            dt_hours,
            wave_ht_ft,
            current_speed_kt,
            current_dir_deg,
        }
    }
}

/// A positional/temporal anchor (mirrors Python `Anchor` dataclass — only the
/// fields used by the drift physics are kept).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anchor {
    pub name: String,
    pub lat: f64,
    pub lon: f64,
    pub anchor_type: String,
    pub confidence: f64,
    pub object_type: String,
    pub windage_factor: f64,
}

/// Output of a Monte-Carlo drift run (mirrors Python `DriftResult`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DriftResult {
    pub mode: String, // "forward" | "backward"
    pub n_particles: usize,
    pub duration_hours: f64,
    pub final_positions: Vec<(f64, f64)>,
    pub centroid: (f64, f64),
    /// 1-σ radius around centroid (mean haversine centroid→final), nm.
    pub spread_nm: f64,
    /// (min_lat, min_lon, max_lat, max_lon)
    pub bbox: (f64, f64, f64, f64),
    pub notes: String,
}

/// Per-object-type windage fraction.
///
/// Mirrors Python `_windage_for_type`:
///   ship 0.03, lifeboat 0.10, life_ring 0.08, body 0.02, debris 0.04, cargo 0.03
pub fn windage_for_type(obj: &str) -> f64 {
    match obj.to_lowercase().as_str() {
        "ship" => 0.03,
        "lifeboat" => 0.10,
        "life_ring" => 0.08,
        "body" => 0.02,
        "debris" => 0.04,
        "cargo" => 0.03,
        _ => 0.03,
    }
}

/// Deterministic Gaussian RNG (seeded LCG + Box-Muller) so runs are repeatable
/// like Python's `random.Random(seed).gauss(...)`.
struct GaussRng {
    state: u64,
    spare: Option<f64>,
}

impl GaussRng {
    fn new(seed: u64) -> Self {
        Self { state: seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1), spare: None }
    }
    fn next_u01(&mut self) -> f64 {
        // SplitMix64-style step.
        self.state = self
            .state
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        // map to (0,1)
        ((z >> 11) as f64 + 0.5) / ((1u64 << 53) as f64)
    }
    /// gauss(mu, sigma) via Box-Muller (caches the spare deviate).
    fn gauss(&mut self, mu: f64, sigma: f64) -> f64 {
        if let Some(s) = self.spare.take() {
            return mu + sigma * s;
        }
        let u1 = self.next_u01().max(1e-12);
        let u2 = self.next_u01();
        let mag = (-2.0 * u1.ln()).sqrt();
        let z0 = mag * (std::f64::consts::TAU * u2).cos();
        let z1 = mag * (std::f64::consts::TAU * u2).sin();
        self.spare = Some(z1);
        mu + sigma * z0
    }
}

/// Core Monte-Carlo drift runner — N particles through the storm phases with
/// 15-minute sub-stepping.
///
/// Faithful port of Python `_run_particles`:
///   * `DT_STEP = 0.25h` sub-stepping (`steps = round(dt_hours / DT_STEP)`)
///   * per-step scatter: `eff_ws = ws * max(0.1, gauss(1, wind_speed_cv))`,
///     `eff_wd = wind_dir + gauss(0, wind_spread_deg)`
///   * current decomposed from (speed_kt, dir_deg)
///   * `direction` = +1 forward, −1 backward
///   * `spread_nm` = mean haversine(centroid, final) (1-σ radius)
#[allow(clippy::too_many_arguments)]
pub fn run_particles(
    start_lat: f64,
    start_lon: f64,
    phases: &[PhysPhase],
    n: usize,
    windage_factor: f64,
    direction: i32,
    wind_spread_deg: f64,
    wind_speed_cv: f64,
    rng_seed: u64,
) -> DriftResult {
    let mut rng = GaussRng::new(rng_seed);
    let mut lats = vec![start_lat; n];
    let mut lons = vec![start_lon; n];
    let total_hours: f64 = phases.iter().map(|p| p.dt_hours).sum();

    for phase in phases {
        let steps = (phase.dt_hours / DT_STEP_HOURS).round() as i64;
        let ws_ms = phase.wind_speed_mph * MPH_TO_MS;
        let cs_ms = phase.current_speed_kt * KT_TO_MS;
        let wave_m = phase.wave_ht_ft * FT_TO_M;
        let cur_dir = phase.current_dir_deg.to_radians();
        let cur_u = cs_ms * cur_dir.sin();
        let cur_v = cs_ms * cur_dir.cos();

        for i in 0..n {
            for _ in 0..steps {
                let eff_ws = ws_ms * 0.1f64.max(rng.gauss(1.0, wind_speed_cv));
                let eff_wd = phase.wind_dir_deg + rng.gauss(0.0, wind_spread_deg);
                let (du, dv) = basic_drift(eff_ws, eff_wd, cur_u, cur_v, wave_m, windage_factor);
                let speed_ms = du.hypot(dv);
                if speed_ms < 1e-9 {
                    continue;
                }
                let drift_dir_deg = (du.atan2(dv).to_degrees() + 360.0) % 360.0;
                let dist_nm = direction as f64 * speed_ms * DT_STEP_HOURS * 3600.0 / NM_TO_M;
                let (nlat, nlon) = destination(lats[i], lons[i], drift_dir_deg, dist_nm);
                lats[i] = nlat;
                lons[i] = nlon;
            }
        }
    }

    let nf = n as f64;
    let clat = lats.iter().sum::<f64>() / nf;
    let clon = lons.iter().sum::<f64>() / nf;
    let spread = lats
        .iter()
        .zip(lons.iter())
        .map(|(&la, &lo)| haversine_nm(clat, clon, la, lo))
        .sum::<f64>()
        / nf;

    let final_positions: Vec<(f64, f64)> = lats.iter().cloned().zip(lons.iter().cloned()).collect();
    let lat_min = lats.iter().cloned().fold(f64::INFINITY, f64::min);
    let lat_max = lats.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let lon_min = lons.iter().cloned().fold(f64::INFINITY, f64::min);
    let lon_max = lons.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    DriftResult {
        mode: if direction == 1 { "forward".into() } else { "backward".into() },
        n_particles: n,
        duration_hours: total_hours,
        final_positions,
        centroid: (clat, clon),
        spread_nm: spread,
        bbox: (lat_min, lon_min, lat_max, lon_max),
        notes: String::new(),
    }
}

/// Forward seeding from a departure point through storm phases.
///
/// Mirrors Python `forward_seed` (windage from object type, `rng_seed=42`,
/// default wind scatter ±20° / CV 0.15).
pub fn forward_seed(
    dep_lat: f64,
    dep_lon: f64,
    phases: &[PhysPhase],
    object_type: &str,
    n_particles: usize,
) -> DriftResult {
    let windage = windage_for_type(object_type);
    run_particles(dep_lat, dep_lon, phases, n_particles, windage, 1, 20.0, 0.15, 42)
}

/// Backward drift from a recovery point to estimate origin.
///
/// Mirrors Python `backward_drift` (direction = −1, `rng_seed=42`).
pub fn backward_drift(
    recovery_lat: f64,
    recovery_lon: f64,
    phases: &[PhysPhase],
    object_type: &str,
    n_particles: usize,
) -> DriftResult {
    let windage = windage_for_type(object_type);
    run_particles(recovery_lat, recovery_lon, phases, n_particles, windage, -1, 20.0, 0.15, 42)
}

/// Result of a candidate→debris consistency check (mirrors the Python dict
/// returned by `consistency_check`).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConsistencyResult {
    pub candidate: (f64, f64),
    pub debris_target: (f64, f64),
    pub predicted_centroid: (f64, f64),
    pub dist_centroid_to_debris_nm: f64,
    pub spread_1sigma_nm: f64,
    pub fraction_within_spread: f64,
    pub consistent: bool,
}

/// Forward-drift a magnetic candidate through the post-sink phases and compare
/// the predicted centroid to a known debris anchor.
///
/// Faithful port of Python `consistency_check`:
///   * post_sink_phases = phases whose label contains "debris" OR dt_hours > 24
///     (falls back to the last phase if none match)
///   * runs `run_particles(..., debris.windage_factor, direction=1, seed=123)`
///   * `fraction_within_spread` = fraction of finals within `spread_nm` of debris
///   * `consistent` = dist(centroid, debris) ≤ 2·spread_nm
pub fn consistency_check(
    candidate_lat: f64,
    candidate_lon: f64,
    debris: &Anchor,
    phases: &[PhysPhase],
    n_particles: usize,
) -> ConsistencyResult {
    let mut post_sink: Vec<PhysPhase> = phases
        .iter()
        .filter(|p| p.label.to_lowercase().contains("debris") || p.dt_hours > 24.0)
        .cloned()
        .collect();
    if post_sink.is_empty() {
        if let Some(last) = phases.last() {
            post_sink.push(last.clone());
        }
    }

    let result = run_particles(
        candidate_lat,
        candidate_lon,
        &post_sink,
        n_particles,
        debris.windage_factor,
        1,
        20.0,
        0.15,
        123,
    );

    let dist_to_debris = haversine_nm(result.centroid.0, result.centroid.1, debris.lat, debris.lon);
    let within = result
        .final_positions
        .iter()
        .filter(|(la, lo)| haversine_nm(*la, *lo, debris.lat, debris.lon) <= result.spread_nm)
        .count();
    let frac = within as f64 / result.n_particles as f64;

    ConsistencyResult {
        candidate: (candidate_lat, candidate_lon),
        debris_target: (debris.lat, debris.lon),
        predicted_centroid: result.centroid,
        dist_centroid_to_debris_nm: (dist_to_debris * 100.0).round() / 100.0,
        spread_1sigma_nm: (result.spread_nm * 100.0).round() / 100.0,
        fraction_within_spread: (frac * 1000.0).round() / 1000.0,
        consistent: dist_to_debris <= 2.0 * result.spread_nm,
    }
}

/// Per-analog summary for an ensemble forward seed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalogSummary {
    pub label: String,
    pub centroid: (f64, f64),
    pub spread_nm: f64,
    pub dist_to_ensemble_nm: f64,
}

/// Ensemble forward-seed summary (mirrors Python `ensemble_forward_seed` dict).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnsembleResult {
    pub ensemble_centroid: (f64, f64),
    pub ensemble_inter_analog_spread_nm: f64,
    pub n_analogs: usize,
    pub per_analog: Vec<AnalogSummary>,
}

/// Run forward seeding for multiple analog storm phase sets and summarise.
///
/// Faithful port of Python `ensemble_forward_seed`: the ensemble centroid is the
/// mean of per-analog centroids; inter-analog spread is the mean distance from
/// the ensemble centroid to each analog centroid.
pub fn ensemble_forward_seed(
    dep_lat: f64,
    dep_lon: f64,
    analog_phase_sets: &[Vec<PhysPhase>],
    analog_labels: &[String],
    object_type: &str,
    n_particles: usize,
) -> Option<EnsembleResult> {
    if analog_phase_sets.is_empty() {
        return None;
    }
    let mut results: Vec<(String, DriftResult)> = Vec::new();
    for (i, phases) in analog_phase_sets.iter().enumerate() {
        let label = analog_labels.get(i).cloned().unwrap_or_default();
        let mut res = forward_seed(dep_lat, dep_lon, phases, object_type, n_particles);
        res.notes = label.clone();
        results.push((label, res));
    }

    let n = results.len() as f64;
    let e_lat = results.iter().map(|(_, r)| r.centroid.0).sum::<f64>() / n;
    let e_lon = results.iter().map(|(_, r)| r.centroid.1).sum::<f64>() / n;
    let inter_spread = results
        .iter()
        .map(|(_, r)| haversine_nm(e_lat, e_lon, r.centroid.0, r.centroid.1))
        .sum::<f64>()
        / n;

    let round2 = |v: f64| (v * 100.0).round() / 100.0;
    let round5 = |v: f64| (v * 100000.0).round() / 100000.0;

    let per_analog = results
        .iter()
        .map(|(label, r)| AnalogSummary {
            label: label.clone(),
            centroid: r.centroid,
            spread_nm: round2(r.spread_nm),
            dist_to_ensemble_nm: round2(haversine_nm(r.centroid.0, r.centroid.1, e_lat, e_lon)),
        })
        .collect();

    Some(EnsembleResult {
        ensemble_centroid: (round5(e_lat), round5(e_lon)),
        ensemble_inter_analog_spread_nm: round2(inter_spread),
        n_analogs: results.len(),
        per_analog,
    })
}

/// A single sensitivity-sweep cell.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SensitivityCell {
    pub speed_scale: f64,
    pub dir_offset_deg: f64,
    pub centroid: (f64, f64),
    pub spread_nm: f64,
}

/// Build a scaled/rotated copy of the three sinking-window phases.
///
/// Mirrors Python `build_analog_phases_from_1909` applied to a caller-supplied
/// base set of (up to) the first three phases.
pub fn build_analog_phases(base: &[PhysPhase], scale_factor: f64, direction_offset: f64) -> Vec<PhysPhase> {
    base.iter()
        .take(3)
        .map(|p| PhysPhase {
            label: format!("{} (scale={:.2} off={:.0}°)", p.label, scale_factor, direction_offset),
            wind_speed_mph: (p.wind_speed_mph * scale_factor * 10.0).round() / 10.0,
            wind_dir_deg: (p.wind_dir_deg + direction_offset).rem_euclid(360.0),
            dt_hours: p.dt_hours,
            wave_ht_ft: (p.wave_ht_ft * scale_factor * 10.0).round() / 10.0,
            current_speed_kt: p.current_speed_kt,
            current_dir_deg: p.current_dir_deg,
        })
        .collect()
}

/// Sweep wind speed ±20% and direction ±20° to bound the sinking zone.
///
/// Faithful port of Python `sensitivity_sweep` (speed scales 0.8/1.0/1.2,
/// direction offsets −20/0/20).  `base_phases` are the sinking-window phases the
/// sweep perturbs.
pub fn sensitivity_sweep(
    dep_lat: f64,
    dep_lon: f64,
    base_phases: &[PhysPhase],
    n_particles: usize,
) -> Vec<SensitivityCell> {
    let mut cells = Vec::new();
    for speed_scale in [0.8, 1.0, 1.2] {
        for dir_offset in [-20.0, 0.0, 20.0] {
            let phases = build_analog_phases(base_phases, speed_scale, dir_offset);
            let res = forward_seed(dep_lat, dep_lon, &phases, "ship", n_particles);
            cells.push(SensitivityCell {
                speed_scale,
                dir_offset_deg: dir_offset,
                centroid: res.centroid,
                spread_nm: (res.spread_nm * 100.0).round() / 100.0,
            });
        }
    }
    cells
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn haversine_zero() {
        assert_relative_eq!(haversine_nm(42.0, -82.0, 42.0, -82.0), 0.0, epsilon = 1e-9);
    }

    #[test]
    fn destination_north() {
        let (lat2, lon2) = destination(42.0, -82.0, 0.0, 1.0); // 1 nm north
        assert!(lat2 > 42.0, "should move north");
        assert_relative_eq!(lon2, -82.0, epsilon = 1e-6);
    }

    #[test]
    fn forward_drift_moves() {
        let storm = StormEvent {
            name: "test".into(),
            phases: vec![StormPhase {
                label: "main".into(),
                wind_speed_ms: 15.0,
                wind_dir_deg: 270.0, // from west → pushes east
                current_u: 0.0,
                current_v: 0.0,
                wave_ht_m: 1.0,
                dt_hours: 6.0,
            }],
        };
        let run = forward_drift_run(DriftParticle { lat: 42.0, lon: -82.0 }, &storm, &DriftConfig::default());
        assert!(run.total_nm > 0.0);
    }

    #[test]
    fn analog_score_identical() {
        let s = analog_storm_score(15.0, 270.0, 12.0, 15.0, 270.0, 12.0);
        assert_relative_eq!(s, 0.0, epsilon = 1e-9);
    }

    // ── historical_drift.py physics port ─────────────────────────────────────

    fn test_phases() -> Vec<PhysPhase> {
        // A short sinking window + a long debris-drift phase (>24h) so
        // consistency_check's post-sink filter selects the long phase.
        vec![
            PhysPhase::new("Pre-storm", 30.0, 270.0, 2.0, 4.0, 0.3, 90.0),
            PhysPhase::new("Storm build", 55.0, 300.0, 2.0, 8.0, 0.5, 120.0),
            PhysPhase::new("Storm peak", 68.0, 315.0, 5.0, 14.0, 0.6, 135.0),
            PhysPhase::new("Post-sink debris drift", 18.0, 285.0, 49.0 * 24.0, 3.0, 0.2, 100.0),
        ]
    }

    #[test]
    fn windage_table_matches_python() {
        assert_eq!(windage_for_type("ship"), 0.03);
        assert_eq!(windage_for_type("lifeboat"), 0.10);
        assert_eq!(windage_for_type("life_ring"), 0.08);
        assert_eq!(windage_for_type("body"), 0.02);
        assert_eq!(windage_for_type("debris"), 0.04);
        assert_eq!(windage_for_type("cargo"), 0.03);
        assert_eq!(windage_for_type("unknown_thing"), 0.03); // default
    }

    #[test]
    fn run_particles_is_deterministic_and_moves() {
        let phases = test_phases();
        let a = run_particles(42.0, -80.5, &phases[..3], 50, 0.03, 1, 20.0, 0.15, 42);
        let b = run_particles(42.0, -80.5, &phases[..3], 50, 0.03, 1, 20.0, 0.15, 42);
        // Same seed → identical centroid (deterministic).
        assert_relative_eq!(a.centroid.0, b.centroid.0, epsilon = 1e-12);
        assert_relative_eq!(a.centroid.1, b.centroid.1, epsilon = 1e-12);
        // The particles drifted away from the start.
        let moved = haversine_nm(42.0, -80.5, a.centroid.0, a.centroid.1);
        assert!(moved > 0.5, "particles should drift, moved {moved}nm");
        // spread_nm is the mean radius (>= 0, and < the bbox-diagonal radius).
        assert!(a.spread_nm >= 0.0);
    }

    #[test]
    fn substepping_count_matches_dt_step() {
        // A 2h phase at DT_STEP=0.25h must be exactly 8 sub-steps. We can't read
        // the internal counter, but a longer phase should drift further than a
        // shorter one with identical forcing — a proxy that sub-stepping scales
        // with dt_hours.
        let short = vec![PhysPhase::new("p", 40.0, 270.0, 1.0, 0.0, 0.0, 0.0)];
        let long = vec![PhysPhase::new("p", 40.0, 270.0, 4.0, 0.0, 0.0, 0.0)];
        let rs = run_particles(42.0, -80.5, &short, 30, 0.05, 1, 0.0, 0.0, 7);
        let rl = run_particles(42.0, -80.5, &long, 30, 0.05, 1, 0.0, 0.0, 7);
        let ds = haversine_nm(42.0, -80.5, rs.centroid.0, rs.centroid.1);
        let dl = haversine_nm(42.0, -80.5, rl.centroid.0, rl.centroid.1);
        assert!(dl > ds * 3.0, "4h phase should drift ~4x the 1h phase: {ds} vs {dl}");
    }

    #[test]
    fn consistency_check_consistent_when_candidate_near_debris() {
        // Forward-drift the candidate through the long debris phase. Place the
        // debris anchor AT the predicted centroid so dist≈0 ≤ 2σ → consistent,
        // and (almost) all particles fall within the 1-σ radius.
        let phases = test_phases();
        let debris0 = Anchor {
            name: "debris".into(),
            lat: 42.0,
            lon: -80.5,
            anchor_type: "debris".into(),
            confidence: 0.5,
            object_type: "life_ring".into(),
            windage_factor: 0.08,
        };
        // First run to find where the candidate actually drifts to.
        let probe = consistency_check(42.4, -80.8, &debris0, &phases, 100);
        // Now anchor the debris at the predicted centroid.
        let debris = Anchor { lat: probe.predicted_centroid.0, lon: probe.predicted_centroid.1, ..debris0 };
        let check = consistency_check(42.4, -80.8, &debris, &phases, 100);

        assert!(check.dist_centroid_to_debris_nm < 0.1, "centroid sits on debris");
        assert!(check.consistent, "should be consistent (dist ≤ 2σ)");
        assert!(
            check.fraction_within_spread > 0.0 && check.fraction_within_spread <= 1.0,
            "fraction within spread in (0,1], got {}",
            check.fraction_within_spread
        );
    }

    #[test]
    fn consistency_check_inconsistent_when_debris_far() {
        let phases = test_phases();
        let debris = Anchor {
            name: "debris".into(),
            lat: 30.0, // far south — nowhere near the Erie drift
            lon: -70.0,
            anchor_type: "debris".into(),
            confidence: 0.5,
            object_type: "life_ring".into(),
            windage_factor: 0.08,
        };
        let check = consistency_check(42.4, -80.8, &debris, &phases, 80);
        assert!(!check.consistent, "far debris should be inconsistent");
        assert!(check.dist_centroid_to_debris_nm > 100.0);
    }

    #[test]
    fn consistency_check_selects_post_sink_phases() {
        // With no >24h phase and no "debris" label, it falls back to the last
        // phase (must still run without panicking and return a result).
        let phases = vec![
            PhysPhase::new("a", 30.0, 270.0, 2.0, 4.0, 0.0, 0.0),
            PhysPhase::new("b", 40.0, 300.0, 2.0, 6.0, 0.0, 0.0),
        ];
        let debris = Anchor {
            name: "d".into(),
            lat: 42.0,
            lon: -80.5,
            anchor_type: "debris".into(),
            confidence: 0.5,
            object_type: "debris".into(),
            windage_factor: 0.04,
        };
        let check = consistency_check(42.1, -80.6, &debris, &phases, 40);
        assert!(check.spread_1sigma_nm >= 0.0);
    }

    #[test]
    fn ensemble_forward_seed_summary() {
        let base = test_phases();
        let a1 = build_analog_phases(&base, 1.0, 0.0);
        let a2 = build_analog_phases(&base, 1.2, 20.0);
        let sets = vec![a1, a2];
        let labels = vec!["analog_1".to_string(), "analog_2".to_string()];
        let ens = ensemble_forward_seed(42.0, -80.5, &sets, &labels, "ship", 50).expect("ensemble");
        assert_eq!(ens.n_analogs, 2);
        assert_eq!(ens.per_analog.len(), 2);
        assert!(ens.ensemble_inter_analog_spread_nm >= 0.0);
    }

    #[test]
    fn sensitivity_sweep_has_nine_cells() {
        // 3 speed scales × 3 direction offsets = 9 cells (matches Python).
        let base = test_phases();
        let cells = sensitivity_sweep(42.0, -80.5, &base, 30);
        assert_eq!(cells.len(), 9);
        let scales: std::collections::BTreeSet<i64> =
            cells.iter().map(|c| (c.speed_scale * 10.0) as i64).collect();
        assert_eq!(scales, [8, 10, 12].into_iter().collect());
    }

    #[test]
    fn backward_drift_runs() {
        let phases = test_phases();
        let bwd = backward_drift(42.14, -80.09, &phases[3..], "body", 50);
        assert_eq!(bwd.mode, "backward");
        assert_eq!(bwd.n_particles, 50);
    }
}
