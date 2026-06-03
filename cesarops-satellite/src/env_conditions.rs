//! Environmental-condition gating for scene selection.
//!
//! The right acquisition CONDITIONS matter as much as the right year. The
//! physics, as practised by the operator:
//!
//! THERMAL REGIME IS SET BY DEPTH vs SUNLIGHT, not a day/night differential of
//! the same wreck:
//!   - A DEEP wreck (below the photic zone / thermocline, ~>200 ft in the Great
//!     Lakes) never sees sunlight — it is ALWAYS a cold sink, day or night, all
//!     season. Its cold plume rising to the thermocline is readable regardless
//!     of time of day. (This is the Andaste regime.)
//!   - A SHALLOW wreck (within the photic zone) HEATS through the day under
//!     sunlight and cools at night — it cycles. For a shallow target you image
//!     at the thermal extreme (peak afternoon heat, or pre-dawn cool).
//!   - The cutoff depth (sunlight penetration) shifts with SEASON (sun angle +
//!     thermocline depth), so the same wreck can be "deep/always-cold" in one
//!     season and "shallow/cycling" in another.
//!
//! PLUMES COME FROM TWO DISTINCT SOURCES:
//!   - SPRING RUNOFF — sediment plumes WITHOUT storm disturbance around them
//!     (cleaner signal, no wind-chop confounding the surface). Runoff also
//!     raises current, and higher current raises the surface ripple/displacement
//!     over structure — itself a surface-readable signal. Often the BEST plume
//!     window.
//!   - POST-STORM — the first calm day after a storm, when storm surge has
//!     suspended lakebed sediment over the wreck but the surface has settled.
//!
//! Weather/season comes from the Open-Meteo archive API (no key); plume/current
//! reasoning is joined to each scene's acquisition date + the site depth.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

// ── Thermal regime (depth vs photic zone) ─────────────────────────────────────

/// Sunlight-penetration depth in the Great Lakes (clear water). Below this a
/// wreck is effectively always cold; above it the wreck cycles with the sun.
/// This is a seasonal nominal — `photic_depth_ft_for_month` refines it.
pub const NOMINAL_PHOTIC_DEPTH_FT: f64 = 200.0;

/// Seasonal photic / thermocline depth (ft). Sun angle and stratification push
/// the warm, lit layer deeper in summer and shallower in spring/fall. Coarse
/// but captures the regime shift: a 150 ft wreck can be "cycling" in August yet
/// "always cold" in April.
pub fn photic_depth_ft_for_month(month: u32) -> f64 {
    match month {
        12 | 1 | 2 => 90.0,   // winter — column near-isothermal/cold, shallow lit layer
        3 | 4 | 5 => 130.0,   // spring — thermocline forming, shallow
        6 | 7 | 8 => 220.0,   // summer — deep warm mixed layer
        9 | 10 | 11 => 160.0, // fall — thermocline deepening then breaking down
        _ => NOMINAL_PHOTIC_DEPTH_FT,
    }
}

/// Thermal regime of a wreck for a given site depth + season.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThermalRegime {
    /// Below the lit/warm layer — a persistent cold sink, readable any time.
    AlwaysCold,
    /// Within the lit layer — heats by day, cools by night (cycles).
    SunCycling,
}

/// Classify a wreck's thermal regime from its depth and the acquisition month.
pub fn thermal_regime(depth_ft: f64, month: u32) -> ThermalRegime {
    if depth_ft >= photic_depth_ft_for_month(month) {
        ThermalRegime::AlwaysCold
    } else {
        ThermalRegime::SunCycling
    }
}

/// Best time-of-day to image a wreck given its regime.
///   - AlwaysCold: any pass works; the cold plume is persistent. Daytime optical
///     (Sentinel-2) is fine for the thermocline-deformation read.
///   - SunCycling: image at a thermal extreme — peak afternoon heat (max
///     positive contrast) or pre-dawn (max cool) — to maximise the signal.
pub fn preferred_pass(regime: ThermalRegime) -> &'static str {
    match regime {
        ThermalRegime::AlwaysCold => "any_persistent_cold_sink",
        ThermalRegime::SunCycling => "thermal_extreme_afternoon_or_predawn",
    }
}

// ── Day condition (ports weather_service.py classify_day_condition) ────────────

/// Per-day weather classification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DayCondition {
    /// Low wind, no precip — best optical baseline / clarity.
    Calm,
    /// Active storm — SAR texture, plume onset, chop confounds optical.
    Storm,
    /// 1–N days after a storm — suspended-sediment plume window.
    PostStorm(u32),
    /// Spring snowmelt/runoff — clean plume + elevated current/ripple, no storm.
    SpringRunoff,
    /// Between calm and storm thresholds.
    Transitional,
}

#[derive(Debug, Clone, Copy)]
pub struct WeatherThresholds {
    pub max_calm_wind: f64,
    pub min_storm_wind: f64,
    pub min_storm_precip: f64,
    pub post_storm_days: u32,
}

impl Default for WeatherThresholds {
    fn default() -> Self {
        Self { max_calm_wind: 15.0, min_storm_wind: 28.0, min_storm_precip: 5.0, post_storm_days: 3 }
    }
}

/// One day of weather (subset of the Open-Meteo archive response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeatherDay {
    pub date: NaiveDate,
    pub wind_speed: f64,
    pub precip_mm: f64,
    #[serde(default)]
    pub cloud_cover: f64,
    /// Optional snowmelt / runoff proxy (mm) when available from the feed.
    #[serde(default)]
    pub snowmelt_mm: f64,
}

/// Is this date in the spring freshet window (snowmelt-driven runoff)?
/// March–May in the Great Lakes; runoff plumes peak with melt + rain.
pub fn is_spring_runoff(day: &WeatherDay) -> bool {
    let m = day.date.month();
    let in_freshet = (3..=5).contains(&m);
    // Calm-ish but with melt/rain feeding sediment — NOT a storm.
    in_freshet && day.wind_speed < 28.0 && (day.snowmelt_mm > 1.0 || day.precip_mm >= 2.0)
}

/// Baseline classify a single day. Spring runoff takes precedence over the
/// calm/transitional label because it is a distinct, desirable plume source.
pub fn classify_day(day: &WeatherDay, t: &WeatherThresholds) -> DayCondition {
    if day.wind_speed >= t.min_storm_wind
        || (day.wind_speed >= 20.0 && day.precip_mm >= t.min_storm_precip)
    {
        DayCondition::Storm
    } else if is_spring_runoff(day) {
        DayCondition::SpringRunoff
    } else if day.wind_speed <= t.max_calm_wind && day.precip_mm < 1.0 {
        DayCondition::Calm
    } else {
        DayCondition::Transitional
    }
}

/// Classify a date series and propagate post-storm tags.
pub fn tag_conditions(days: &[WeatherDay], t: &WeatherThresholds) -> Vec<DayCondition> {
    let mut cond: Vec<DayCondition> = days.iter().map(|d| classify_day(d, t)).collect();
    let storm_dates: Vec<NaiveDate> = days
        .iter()
        .enumerate()
        .filter(|(i, _)| cond[*i] == DayCondition::Storm)
        .map(|(_, d)| d.date)
        .collect();
    for sdate in &storm_dates {
        for offset in 1..=t.post_storm_days {
            let target = *sdate + chrono::Duration::days(offset as i64);
            if let Some(idx) = days.iter().position(|d| d.date == target) {
                match cond[idx] {
                    DayCondition::Storm | DayCondition::SpringRunoff => continue, // don't override
                    DayCondition::PostStorm(o) if o <= offset => continue,
                    _ => cond[idx] = DayCondition::PostStorm(offset),
                }
            }
        }
    }
    cond
}

/// First calm day after a storm (post-storm plume window).
pub fn first_calm_after_storm(days: &[WeatherDay], t: &WeatherThresholds) -> Option<NaiveDate> {
    let cond = tag_conditions(days, t);
    days.iter()
        .zip(cond.iter())
        .find(|(_, c)| matches!(c, DayCondition::PostStorm(1)))
        .map(|(d, _)| d.date)
}

/// Spring-runoff plume days (clean plume, elevated current/ripple, no storm).
pub fn spring_runoff_days(days: &[WeatherDay], t: &WeatherThresholds) -> Vec<NaiveDate> {
    days.iter()
        .zip(tag_conditions(days, t).iter())
        .filter(|(_, c)| **c == DayCondition::SpringRunoff)
        .map(|(d, _)| d.date)
        .collect()
}

// ── Condition → intent suitability ─────────────────────────────────────────────

/// 0–1 suitability of a day's condition for a scan intent. The selector
/// multiplies this into cloud ranking so the right physics window wins.
pub fn condition_suitability(
    cond: DayCondition,
    intent: crate::lake_levels::ScanIntent,
) -> f64 {
    use crate::lake_levels::ScanIntent::*;
    use DayCondition::*;
    match intent {
        // Plume / spill work: spring runoff is the cleanest source, then the
        // first calm day after a storm.
        EventResponse => match cond {
            SpringRunoff => 1.0,
            PostStorm(1) => 0.9,
            PostStorm(2) => 0.7,
            PostStorm(3) => 0.5,
            PostStorm(_) => 0.4,
            Calm => 0.4,
            Transitional => 0.3,
            Storm => 0.1,
        },
        // Clarity & low-water wreck work want flat-calm clear water; runoff
        // turbidity HURTS clarity, so it scores low here.
        ZebraClarity | LowWaterWreck => match cond {
            Calm => 1.0,
            Transitional => 0.5,
            PostStorm(_) => 0.4,
            SpringRunoff => 0.2,
            Storm => 0.0,
        },
        RecentSinking | Generic => match cond {
            Calm => 1.0,
            Transitional => 0.6,
            SpringRunoff => 0.5,
            PostStorm(_) => 0.5,
            Storm => 0.2,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lake_levels::ScanIntent;

    fn d(y: i32, m: u32, dd: u32, wind: f64, precip: f64) -> WeatherDay {
        WeatherDay { date: NaiveDate::from_ymd_opt(y, m, dd).unwrap(), wind_speed: wind, precip_mm: precip, cloud_cover: 0.0, snowmelt_mm: 0.0 }
    }

    #[test]
    fn deep_wreck_is_always_cold_regardless_of_season() {
        // A 460 ft wreck (Andaste regime) is below the lit layer every month.
        for m in 1..=12 {
            assert_eq!(thermal_regime(460.0, m), ThermalRegime::AlwaysCold, "month {m}");
        }
        assert_eq!(preferred_pass(ThermalRegime::AlwaysCold), "any_persistent_cold_sink");
    }

    #[test]
    fn shallow_wreck_cycles_but_regime_shifts_with_season() {
        // A 150 ft wreck: within the summer warm layer (220 ft) → cycling, but
        // below the spring lit layer (130 ft) → always-cold in April.
        assert_eq!(thermal_regime(150.0, 7), ThermalRegime::SunCycling);   // summer
        assert_eq!(thermal_regime(150.0, 4), ThermalRegime::AlwaysCold);   // spring
        assert_eq!(preferred_pass(ThermalRegime::SunCycling), "thermal_extreme_afternoon_or_predawn");
    }

    #[test]
    fn spring_runoff_detected_and_distinct_from_storm() {
        let mut day = d(2023, 4, 15, 12.0, 3.0); // April, calm-ish, rain
        day.snowmelt_mm = 4.0;
        assert!(is_spring_runoff(&day));
        assert_eq!(classify_day(&day, &WeatherThresholds::default()), DayCondition::SpringRunoff);
        // Same conditions in July are NOT spring runoff.
        let mut july = day.clone();
        july.date = NaiveDate::from_ymd_opt(2023, 7, 15).unwrap();
        assert_ne!(classify_day(&july, &WeatherThresholds::default()), DayCondition::SpringRunoff);
    }

    #[test]
    fn post_storm_first_calm() {
        let t = WeatherThresholds::default();
        let days = vec![
            d(2023, 7, 5, 30.0, 8.0), // storm
            d(2023, 7, 6, 8.0, 0.0),  // post_storm_1
            d(2023, 7, 7, 6.0, 0.0),  // post_storm_2
        ];
        assert_eq!(first_calm_after_storm(&days, &t).unwrap(), NaiveDate::from_ymd_opt(2023, 7, 6).unwrap());
    }

    #[test]
    fn plume_intent_prefers_spring_runoff_over_post_storm() {
        let runoff = condition_suitability(DayCondition::SpringRunoff, ScanIntent::EventResponse);
        let ps1 = condition_suitability(DayCondition::PostStorm(1), ScanIntent::EventResponse);
        let storm = condition_suitability(DayCondition::Storm, ScanIntent::EventResponse);
        assert!(runoff > ps1 && ps1 > storm, "runoff {runoff} > post-storm {ps1} > storm {storm}");
        // Clarity work penalises runoff turbidity.
        assert!(condition_suitability(DayCondition::Calm, ScanIntent::ZebraClarity)
            > condition_suitability(DayCondition::SpringRunoff, ScanIntent::ZebraClarity));
    }
}

// ── Scan-intent condition matrix ─────────────────────────────────────────────
//
// Each detection target has an IDEAL acquisition condition. This encodes the
// operator's full matrix (wind/cloud/days-since-storm/turbidity/season) so
// scene selection can SCORE a candidate scene's fitness for a given intent,
// using buoy wave/wind (see `buoy.rs`) + weather + turbidity.

/// What the operator is trying to detect on this pass. Drives which scenes
/// (calm vs storm, clear vs turbid, which season) score highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanIntent {
    /// Shallow-water depth from blue/green. Wants glassy calm, clear, low turbidity.
    Bathymetry,
    /// Zebra/quagga clarity anomaly. Calm clear days, summer.
    ZebraClarity,
    /// Sediment plume. 0–2 days AFTER a 15–30 mph wind/storm; clear during pass.
    SedimentPlume,
    /// Thermal fronts / cold upwelling. Clear pass 1–3 days after offshore wind.
    ThermalFront,
    /// Heat pattern (warm shallow). Clear sunny days after several warm days.
    HeatPattern,
    /// Hydrocarbon film (NIR/SWIR). Calm, light wind 2–8 mph, dry, clear.
    Hydrocarbon,
    /// Sun glint / surface roughness / current. Cloud-free, predictable sun.
    SunGlint,
    /// Generic deep-wreck cold-sink / plume column (the Burns/Andaste case).
    DeepWreck,
}

impl ScanIntent {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_lowercase().as_str() {
            "bathymetry" | "bathy" => Self::Bathymetry,
            "zebra_clarity" | "clarity" => Self::ZebraClarity,
            "sediment_plume" | "plume" => Self::SedimentPlume,
            "thermal_front" | "upwelling" => Self::ThermalFront,
            "heat_pattern" | "heat" => Self::HeatPattern,
            "hydrocarbon" | "oil" | "fuel" => Self::Hydrocarbon,
            "sun_glint" | "glint" => Self::SunGlint,
            "deep_wreck" | "cold_sink" => Self::DeepWreck,
            _ => return None,
        })
    }
}

/// Conditions observed for a scene's acquisition date, assembled from buoy +
/// weather + (optionally) turbidity. Any field may be missing.
#[derive(Debug, Clone, Default)]
pub struct SceneConditions {
    pub wind_speed_ms: Option<f64>,
    pub wave_height_m: Option<f64>,
    pub cloud_pct: Option<f64>,
    pub days_since_storm: Option<i32>,
    /// Turbidity proxy (e.g. NTU or a relative index). Lower = clearer.
    pub turbidity: Option<f64>,
    pub month: u32,
}

/// Score 0–1 how well a scene's conditions fit the intent. 1 = ideal.
/// Missing fields are treated neutrally (don't penalize what we can't measure).
pub fn intent_fitness(intent: ScanIntent, c: &SceneConditions) -> f64 {
    let wind = c.wind_speed_ms;
    let wave = c.wave_height_m;
    let cloud = c.cloud_pct.unwrap_or(0.0);
    let dss = c.days_since_storm;
    let turb = c.turbidity;
    let mph = |ms: f64| ms * 2.23694; // m/s → mph

    // Cloud penalty is near-universal (radar/altimetry intents excepted, but
    // these are all optical/thermal).
    let cloud_ok = if cloud <= 10.0 { 1.0 } else { (1.0 - (cloud - 10.0) / 90.0).max(0.0) };

    // Calm score: low wind + low wave.
    let calm = {
        let w = wind.map(|x| {
            let m = mph(x);
            if m <= 8.0 { 1.0 } else { (1.0 - (m - 8.0) / 22.0).max(0.0) }
        }).unwrap_or(0.7);
        let h = wave.map(|x| if x <= 0.3 { 1.0 } else { (1.0 - (x - 0.3) / 1.2).max(0.0) }).unwrap_or(0.7);
        0.5 * w + 0.5 * h
    };

    // Clarity score: low turbidity.
    let clear_water = turb.map(|t| (1.0 - (t / 10.0)).clamp(0.0, 1.0)).unwrap_or(0.7);

    // Storm-recency helpers.
    let after_storm_window = |lo: i32, hi: i32| -> f64 {
        match dss {
            Some(d) if d >= lo && d <= hi => 1.0,
            Some(d) if d < lo => 0.3, // too soon (still rough)
            Some(d) => (1.0 - (d - hi) as f64 / 7.0).max(0.2), // settling/faded
            None => 0.6,
        }
    };

    let s = match intent {
        ScanIntent::Bathymetry => {
            // glassy calm + clear + low turbidity + 3–7 days post-storm
            0.30 * calm + 0.25 * clear_water + 0.20 * cloud_ok + 0.25 * after_storm_window(3, 7)
        }
        ScanIntent::ZebraClarity => {
            0.30 * calm + 0.30 * clear_water + 0.20 * cloud_ok + 0.20 * after_storm_window(3, 7)
        }
        ScanIntent::SedimentPlume => {
            // 0–2 days AFTER storm, clear during pass; calm during pass NOT required
            let recency = match dss {
                Some(d) if d <= 2 => 1.0,
                Some(d) => (1.0 - (d - 2) as f64 / 5.0).max(0.0),
                None => 0.4,
            };
            0.55 * recency + 0.45 * cloud_ok
        }
        ScanIntent::ThermalFront => {
            // clear pass 1–3 days after offshore wind
            0.35 * cloud_ok + 0.40 * after_storm_window(1, 3) + 0.25 * calm
        }
        ScanIntent::HeatPattern => {
            // clear sunny days after several warm days; calm-ish
            0.45 * cloud_ok + 0.30 * calm + 0.25 * after_storm_window(3, 10)
        }
        ScanIntent::Hydrocarbon => {
            0.40 * calm + 0.35 * cloud_ok + 0.25 * clear_water
        }
        ScanIntent::SunGlint => {
            // cloud-free; some surface roughness is fine (don't over-reward calm)
            0.65 * cloud_ok + 0.35 * wave.map(|x| if x < 0.6 { 1.0 } else { 0.5 }).unwrap_or(0.7)
        }
        ScanIntent::DeepWreck => {
            // the cold-sink/plume column: calm + clear + clear water, stable
            0.35 * calm + 0.25 * clear_water + 0.25 * cloud_ok + 0.15 * after_storm_window(2, 7)
        }
    };
    s.clamp(0.0, 1.0)
}

/// Ideal month window per intent (Great Lakes), used to pre-filter the archive.
pub fn intent_season_months(intent: ScanIntent) -> &'static [u32] {
    match intent {
        ScanIntent::Bathymetry => &[5, 6, 9, 10],
        ScanIntent::ZebraClarity => &[7, 8, 9],
        ScanIntent::SedimentPlume => &[3, 4, 5],
        ScanIntent::ThermalFront => &[6, 7, 8, 9],
        ScanIntent::HeatPattern => &[6, 7, 8],
        ScanIntent::Hydrocarbon => &[4, 5, 6, 7, 8, 9, 10],
        ScanIntent::SunGlint => &[4, 5, 6, 7, 8, 9, 10],
        ScanIntent::DeepWreck => &[6, 7, 8, 9],
    }
}

// ── Composite download-priority formula (operator's chat-paste spec) ──────────
//
//   download_priority = thermal + clarity + stratification + calm_water
//                       + post_storm + seasonal
//
// Each term is 0–1; the sum (0–6) is normalised to 0–1. A scene is archived
// only if it clears a threshold. Terms map to the operator's signal-type table:
//   thermal        — cold-night→sunny-day contrast + stable column (cold-sink)
//   clarity        — low turbidity / clear water (mussel-filtered, bloom-free)
//   stratification — stable summer/early-fall column (no turnover mixing)
//   calm_water     — low wind + low wave
//   post_storm     — 12–72 h after storm (delayed window scores highest)
//   seasonal       — early-spring (post ice-out) and FALL score highest

/// Great Lakes seasonal priority (operator: spring AND fall are the windows;
/// fall often the single highest-priority download window).
pub fn seasonal_score(month: u32) -> f64 {
    match month {
        // Fall: peak mussel clarity + storm plumes + cold-night thermal contrast.
        9 | 10 => 1.0,
        11 => 0.8,
        // Early spring after ice-out: clarity, low bio noise, baseline roughness.
        4 | 5 => 0.9,
        6 => 0.7,
        // Mid-summer: stratification good but biological productivity rises.
        7 | 8 => 0.6,
        // Winter / ice / turnover: low priority.
        _ => 0.2,
    }
}

/// Stratification stability score: stable column (no turnover) helps thermal /
/// thermocline signatures. Summer–early-fall are most stable; spring/fall
/// turnover periods are penalised. `days_since_storm` (wind mixing) refines it.
pub fn stratification_score(c: &SceneConditions) -> f64 {
    let season = match c.month {
        7 | 8 => 1.0,       // peak summer stratification
        6 | 9 => 0.8,
        5 | 10 => 0.5,      // shoulder; turnover risk
        11 | 4 => 0.3,      // turnover periods
        _ => 0.2,
    };
    // Recent wind mixing degrades stratification.
    let mixing = match c.days_since_storm {
        Some(d) if d >= 3 => 1.0,
        Some(d) if d >= 1 => 0.6,
        Some(_) => 0.3,
        None => 0.8,
    };
    0.6 * season + 0.4 * mixing
}

/// The composite download-priority score (0–1) for a scene, per the operator's
/// formula. `cold_night_contrast` is an optional 0–1 thermal-gate input (clear
/// nights + light winds + strong air-water ΔT); when absent it's inferred from
/// calm + clear + season.
pub fn download_priority(c: &SceneConditions, cold_night_contrast: Option<f64>) -> f64 {
    let mph = |ms: f64| ms * 2.23694;
    let cloud = c.cloud_pct.unwrap_or(0.0);
    let cloud_ok = if cloud <= 10.0 { 1.0 } else { (1.0 - (cloud - 10.0) / 90.0).max(0.0) };

    let calm_water = {
        let w = c.wind_speed_ms.map(|x| {
            let m = mph(x);
            if m <= 8.0 { 1.0 } else { (1.0 - (m - 8.0) / 22.0).max(0.0) }
        }).unwrap_or(0.7);
        let h = c.wave_height_m.map(|x| if x <= 0.3 { 1.0 } else { (1.0 - (x - 0.3) / 1.2).max(0.0) }).unwrap_or(0.7);
        0.5 * w + 0.5 * h
    };

    let clarity = c.turbidity.map(|t| (1.0 - t / 10.0).clamp(0.0, 1.0)).unwrap_or(0.7) * cloud_ok;

    // Post-storm: delayed 24–72 h window scores highest (organised signal),
    // immediate 0–24 h still useful, long-calm fades.
    let post_storm = match c.days_since_storm {
        Some(d) if (1..=3).contains(&d) => 1.0,  // 24–72 h: best
        Some(0) => 0.7,                          // immediate
        Some(d) if d <= 7 => (1.0 - (d - 3) as f64 / 8.0).max(0.3),
        Some(_) => 0.2,
        None => 0.5,
    };

    let stratification = stratification_score(c);
    let seasonal = seasonal_score(c.month);

    // Thermal: cold-night→sunny-day contrast + stable column. If a measured
    // contrast is supplied use it; else infer from calm + clear + cold season.
    let thermal = cold_night_contrast.unwrap_or_else(|| {
        let cold_season = matches!(c.month, 4 | 5 | 9 | 10 | 11);
        0.5 * calm_water + 0.3 * cloud_ok + if cold_season { 0.2 } else { 0.0 }
    });

    let sum = thermal + clarity + stratification + calm_water + post_storm + seasonal;
    (sum / 6.0).clamp(0.0, 1.0)
}

/// Per-signal breakdown of a scene's suitability — the granular form of
/// `download_priority`. Instead of collapsing to one scalar, expose each
/// sub-score so (a) the operator/engine can weight them per intent and (b) the
/// values become ML features later. `confidence` reflects how much real data
/// (vs neutral defaults) backed the scores.
#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
pub struct SceneScore {
    pub clarity: f64,
    pub thermal: f64,
    pub plume: f64,
    pub glint: f64,
    pub roughness: f64,
    pub calm: f64,
    pub seasonal: f64,
    /// Composite (same value as `download_priority`) for convenience.
    pub priority: f64,
    /// 0–1: fraction of inputs that were actually measured (not neutral).
    pub confidence: f64,
}

/// Compute the per-signal `SceneScore` for a scene's conditions.
pub fn scene_score(c: &SceneConditions, cold_night_contrast: Option<f64>) -> SceneScore {
    let mph = |ms: f64| ms * 2.23694;
    let cloud = c.cloud_pct.unwrap_or(0.0);
    let cloud_ok = if cloud <= 10.0 { 1.0 } else { (1.0 - (cloud - 10.0) / 90.0).max(0.0) };

    let calm = {
        let w = c.wind_speed_ms.map(|x| {
            let m = mph(x);
            if m <= 8.0 { 1.0 } else { (1.0 - (m - 8.0) / 22.0).max(0.0) }
        }).unwrap_or(0.7);
        let h = c.wave_height_m.map(|x| if x <= 0.3 { 1.0 } else { (1.0 - (x - 0.3) / 1.2).max(0.0) }).unwrap_or(0.7);
        0.5 * w + 0.5 * h
    };

    let clarity = c.turbidity.map(|t| (1.0 - t / 10.0).clamp(0.0, 1.0)).unwrap_or(0.7) * cloud_ok;

    // Plume favours recent storm/runoff; glint favours clear + some roughness;
    // roughness is the current-modulation surface signal (moderate wind helps).
    let plume = match c.days_since_storm {
        Some(d) if (1..=3).contains(&d) => 1.0,
        Some(0) => 0.7,
        Some(d) if d <= 7 => (1.0 - (d - 3) as f64 / 8.0).max(0.2),
        Some(_) => 0.2,
        None => 0.4,
    };
    let glint = 0.7 * cloud_ok + 0.3 * c.wave_height_m.map(|x| if x < 0.6 { 1.0 } else { 0.5 }).unwrap_or(0.7);
    let roughness = c.wind_speed_ms.map(|x| {
        let m = mph(x);
        // moderate wind (6-15 mph) best for current-roughness expression
        if (6.0..=15.0).contains(&m) { 1.0 } else if m < 6.0 { 0.5 } else { (1.0 - (m - 15.0) / 20.0).max(0.2) }
    }).unwrap_or(0.6);
    let seasonal = seasonal_score(c.month);
    let thermal = cold_night_contrast.unwrap_or_else(|| {
        let cold_season = matches!(c.month, 4 | 5 | 9 | 10 | 11);
        0.5 * calm + 0.3 * cloud_ok + if cold_season { 0.2 } else { 0.0 }
    });

    let confidence = {
        let n = [c.wind_speed_ms.is_some(), c.wave_height_m.is_some(),
                 c.cloud_pct.is_some(), c.days_since_storm.is_some(),
                 c.turbidity.is_some()].iter().filter(|&&b| b).count();
        n as f64 / 5.0
    };

    SceneScore {
        clarity, thermal, plume, glint, roughness, calm, seasonal,
        priority: download_priority(c, cold_night_contrast),
        confidence,
    }
}

// ── Temporal Isolation Gate (operator's chat-paste spec) ─────────────────────
//
// "Was the scene different from the previous N good scenes?" — we want ~100
// scenes from DISTINCT environmental states, not thousands of near-duplicates
// (which waste storage and burn the P100s on redundant data). A candidate is
// accepted only if its environmental state vector is far enough from every
// recently-accepted scene.

/// Normalised environmental-state vector for a scene (each ~0–1) used to judge
/// whether two scenes are environmentally redundant.
pub fn state_vector(c: &SceneConditions) -> [f64; 5] {
    let wind = c.wind_speed_ms.map(|x| (x / 15.0).min(1.0)).unwrap_or(0.5);
    let wave = c.wave_height_m.map(|x| (x / 2.0).min(1.0)).unwrap_or(0.5);
    let cloud = (c.cloud_pct.unwrap_or(0.0) / 100.0).clamp(0.0, 1.0);
    let turb = c.turbidity.map(|t| (t / 10.0).clamp(0.0, 1.0)).unwrap_or(0.5);
    // Day-of-year phase captures season; use a 0–1 ramp across the year.
    let season = (c.month.clamp(1, 12) as f64 - 1.0) / 11.0;
    [wind, wave, cloud, turb, season]
}

/// Euclidean distance between two scene state vectors.
pub fn state_distance(a: &SceneConditions, b: &SceneConditions) -> f64 {
    let va = state_vector(a);
    let vb = state_vector(b);
    va.iter().zip(vb.iter()).map(|(x, y)| (x - y).powi(2)).sum::<f64>().sqrt()
}

/// Decide whether `candidate` is environmentally DISTINCT from all
/// `accepted` scenes. `min_distance` is the isolation radius (state-vector
/// units; ~0.15–0.25 is a reasonable "meaningfully different" threshold).
pub fn is_temporally_isolated(
    candidate: &SceneConditions,
    accepted: &[SceneConditions],
    min_distance: f64,
) -> bool {
    accepted.iter().all(|a| state_distance(candidate, a) >= min_distance)
}

/// Greedily select up to `target` environmentally-distinct scenes from
/// `scored` (each `(scene_conditions, priority_score)`), highest-priority
/// first, keeping a new scene only if it is at least `min_distance` from every
/// already-kept scene. Returns the kept indices into `scored`.
///
/// This is the "100 excellent scenes from distinct states, not 5000 near-dupes"
/// selector. Pair it with `download_priority` for the score.
pub fn select_distinct_scenes(
    scored: &[(SceneConditions, f64)],
    target: usize,
    min_distance: f64,
) -> Vec<usize> {
    let mut order: Vec<usize> = (0..scored.len()).collect();
    order.sort_by(|&a, &b| {
        scored[b].1.partial_cmp(&scored[a].1).unwrap_or(std::cmp::Ordering::Equal)
    });
    let mut kept: Vec<usize> = Vec::new();
    let mut kept_cond: Vec<SceneConditions> = Vec::new();
    for i in order {
        if kept.len() >= target {
            break;
        }
        let (ref cond, _score) = scored[i];
        if is_temporally_isolated(cond, &kept_cond, min_distance) {
            kept.push(i);
            kept_cond.push(cond.clone());
        }
    }
    kept
}

#[cfg(test)]
mod intent_tests {
    use super::*;

    #[test]
    fn bathymetry_prefers_calm_clear() {
        let calm = SceneConditions {
            wind_speed_ms: Some(2.0), wave_height_m: Some(0.1), cloud_pct: Some(2.0),
            days_since_storm: Some(5), turbidity: Some(1.0), month: 6,
        };
        let rough = SceneConditions {
            wind_speed_ms: Some(10.0), wave_height_m: Some(1.2), cloud_pct: Some(60.0),
            days_since_storm: Some(0), turbidity: Some(8.0), month: 6,
        };
        let fc = intent_fitness(ScanIntent::Bathymetry, &calm);
        let fr = intent_fitness(ScanIntent::Bathymetry, &rough);
        assert!(fc > 0.85, "calm clear should score high, got {fc}");
        assert!(fr < 0.4, "rough turbid should score low, got {fr}");
        assert!(fc > fr);
    }

    #[test]
    fn sediment_plume_wants_recent_storm() {
        let post_storm = SceneConditions {
            cloud_pct: Some(5.0), days_since_storm: Some(1), month: 4, ..Default::default()
        };
        let long_calm = SceneConditions {
            cloud_pct: Some(5.0), days_since_storm: Some(14), month: 4, ..Default::default()
        };
        assert!(intent_fitness(ScanIntent::SedimentPlume, &post_storm)
            > intent_fitness(ScanIntent::SedimentPlume, &long_calm));
    }

    #[test]
    fn scene_score_breaks_out_signals_and_confidence() {
        // Full data → confidence 1.0; fall + calm + clear scores high priority.
        let full = SceneConditions {
            wind_speed_ms: Some(3.0), wave_height_m: Some(0.1), cloud_pct: Some(3.0),
            days_since_storm: Some(2), turbidity: Some(1.0), month: 10,
        };
        let s = super::scene_score(&full, None);
        assert!((s.confidence - 1.0).abs() < 1e-9);
        assert!(s.plume > 0.9, "d2 storm = peak plume, got {}", s.plume);
        assert!(s.seasonal > 0.9, "October = peak season, got {}", s.seasonal);
        assert!(s.priority > 0.6);
        // Sparse data → lower confidence.
        let sparse = SceneConditions { month: 10, ..Default::default() };
        assert!(super::scene_score(&sparse, None).confidence < 0.2);
    }

    #[test]
    fn intent_parse_roundtrip() {
        assert_eq!(ScanIntent::parse("bathy"), Some(ScanIntent::Bathymetry));
        assert_eq!(ScanIntent::parse("plume"), Some(ScanIntent::SedimentPlume));
        assert_eq!(ScanIntent::parse("cold_sink"), Some(ScanIntent::DeepWreck));
        assert_eq!(ScanIntent::parse("nonsense"), None);
    }
}
