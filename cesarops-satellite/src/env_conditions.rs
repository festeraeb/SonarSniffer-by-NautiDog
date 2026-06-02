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
