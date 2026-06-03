//! Acquisition planning — the knob an LLM / n8n worker turns to choose HOW to
//! build a scene stack for a target, then drives batched scene qualification.
//!
//! Workflow (operator's spec):
//!   1. MODE: BeforeAfter (recent sinking within the satellite-derived years)
//!      vs Historical (defaults to the available-data years for each needed
//!      sensor).
//!   2. YEAR SELECTION: Historical → `lake_levels::select_years` per sensor.
//!      BeforeAfter → bracket the sink date with pre- and post-event windows.
//!   3. CONDITION QUERY: for each candidate scene date, query weather +
//!      satellite pass day/night geometry, then check against buoy wave/wind
//!      (`buoy.rs`) and the scan-intent fitness matrix (`env_conditions.rs`).
//!   4. BATCHED ACQUISITION: download (or pull from local archive) a first
//!      batch, qualify against the gate, then batch more until ~100 qualifying
//!      scenes (TARGET_SCENES), with MIN_SCENES (20) required to begin scans.

use serde::{Deserialize, Serialize};

/// Minimum qualifying scenes before any persistence scan is trustworthy
/// (operator gold standard).
pub const MIN_SCENES: usize = 20;
/// Target qualifying scenes to accumulate for a full-confidence run.
pub const TARGET_SCENES: usize = 100;
/// Scenes to request per acquisition batch.
pub const BATCH_SIZE: usize = 30;

/// How the operator wants the stack built.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScanMode {
    /// A recent sinking that falls within the satellite-derived archive years.
    /// Brackets the event date: a PRE window (last clean look before) and a
    /// POST window (first looks after) so the wreck appears in the "after" only.
    BeforeAfter,
    /// A historical wreck (predates or spans the archive). Defaults to the
    /// best available-data years per sensor, ranked by intent.
    Historical,
}

impl ScanMode {
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "before_after" | "beforeafter" | "before-after" | "recent" => Some(Self::BeforeAfter),
            "historical" | "hist" => Some(Self::Historical),
            _ => None,
        }
    }
}

/// A planned acquisition window for one sensor family.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AcqWindow {
    pub sensor: String,
    /// Inclusive [start, end] ISO dates.
    pub start: String,
    pub end: String,
    pub label: String, // "pre_event" | "post_event" | "<year>" | "clarity"
}

/// The full acquisition plan an orchestrator executes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanPlan {
    pub mode: String,
    pub intent: String,
    pub windows: Vec<AcqWindow>,
    pub min_scenes: usize,
    pub target_scenes: usize,
    pub batch_size: usize,
    /// Whether day AND night passes are both wanted (thermal targets) or just day.
    pub want_night_pass: bool,
    pub notes: String,
}

/// Build the acquisition plan.
///
/// `sink_date`: required for BeforeAfter (YYYY-MM-DD); ignored for Historical.
/// `sensors`: families to acquire ("sentinel2", "landsat", "icesat2", ...).
/// `intent`: drives season windows + which conditions qualify a scene.
pub fn build_plan(
    mode: ScanMode,
    lat: f64,
    lon: f64,
    intent: crate::env_conditions::ScanIntent,
    sensors: &[&str],
    sink_date: Option<&str>,
    want_night_pass: bool,
) -> ScanPlan {
    use crate::lake_levels::{select_years, ScanIntent as LlIntent, SensorFamily};

    let mut windows = Vec::new();
    let season = crate::env_conditions::intent_season_months(intent);

    match mode {
        ScanMode::BeforeAfter => {
            // Bracket the event: 2 years pre, everything post up to now.
            if let Some(sd) = sink_date {
                if let Ok(d) = chrono::NaiveDate::parse_from_str(sd, "%Y-%m-%d") {
                    use chrono::Datelike;
                    for s in sensors {
                        // PRE: 2 seasons before the sink, same season window.
                        let pre_start = format!("{}-01-01", d.year() - 2);
                        let pre_end = format!("{}-12-31", d.year() - 1);
                        windows.push(AcqWindow {
                            sensor: s.to_string(),
                            start: pre_start,
                            end: pre_end,
                            label: "pre_event".into(),
                        });
                        // POST: from the sink date forward to now.
                        let now = chrono::Local::now().naive_local().date();
                        windows.push(AcqWindow {
                            sensor: s.to_string(),
                            start: sd.to_string(),
                            end: now.format("%Y-%m-%d").to_string(),
                            label: "post_event".into(),
                        });
                    }
                }
            }
        }
        ScanMode::Historical => {
            // Per sensor: pick the best available-data years for the intent.
            let ll_intent = LlIntent::from_str(&intent_to_ll(intent));
            for s in sensors {
                let fam = SensorFamily::from_str(s);
                let mut years = select_years(lat, lon, fam, ll_intent, 8);
                // Fallback: if the intent's preferred years predate this
                // sensor's archive (e.g. Sentinel-2 can't reach 2012-13 lows),
                // take the most-recent available years instead so the stack
                // still gets built (deep-wreck persistence works on any years).
                if years.is_empty() {
                    years = select_years(lat, lon, fam, LlIntent::Generic, 8);
                }
                for y in years {
                    // Restrict each year to the intent's season months.
                    let (m0, m1) = season_bounds(season);
                    windows.push(AcqWindow {
                        sensor: s.to_string(),
                        start: format!("{y}-{m0:02}-01"),
                        end: format!("{y}-{m1:02}-28"),
                        label: y.to_string(),
                    });
                }
            }
        }
    }

    ScanPlan {
        mode: format!("{mode:?}"),
        intent: format!("{intent:?}"),
        windows,
        min_scenes: MIN_SCENES,
        target_scenes: TARGET_SCENES,
        batch_size: BATCH_SIZE,
        want_night_pass,
        notes: "Qualify each scene: weather + day/night pass geometry + buoy \
                wave/wind + intent fitness. Batch until target_scenes qualify; \
                begin scans at min_scenes."
            .into(),
    }
}

/// Map env_conditions::ScanIntent → lake_levels intent string for year ranking.
fn intent_to_ll(intent: crate::env_conditions::ScanIntent) -> String {
    use crate::env_conditions::ScanIntent::*;
    match intent {
        ZebraClarity => "zebra_clarity",
        SedimentPlume => "event_response",
        Bathymetry => "low_water_wreck",
        DeepWreck | ThermalFront | HeatPattern => "low_water_wreck",
        Hydrocarbon | SunGlint => "generic",
    }
    .to_string()
}

fn season_bounds(months: &[u32]) -> (u32, u32) {
    if months.is_empty() {
        (1, 12)
    } else {
        (*months.iter().min().unwrap(), *months.iter().max().unwrap())
    }
}

/// Per-scene qualification verdict. The orchestrator collects these until it has
/// `min_scenes` passing (to start) and keeps batching toward `target_scenes`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneVerdict {
    pub date: String,
    pub sensor: String,
    pub qualifies: bool,
    pub fitness: f64,
    pub wave_height_m: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub cloud_pct: Option<f64>,
    pub reason: String,
}

/// Qualify one candidate scene against the gate: combines buoy calm-state with
/// the scan-intent fitness. `min_fitness` is the pass threshold (e.g. 0.6).
pub fn qualify_scene(
    date: &str,
    sensor: &str,
    intent: crate::env_conditions::ScanIntent,
    cond: &crate::env_conditions::SceneConditions,
    buoy_calm: Option<bool>,
    min_fitness: f64,
) -> SceneVerdict {
    let fitness = crate::env_conditions::intent_fitness(intent, cond);
    // A scene qualifies if intent fitness clears the bar AND (when we have buoy
    // data) the water was calm. Missing buoy data doesn't auto-fail (fitness
    // already incorporates wind/wave when present).
    let calm_ok = buoy_calm.unwrap_or(true);
    let qualifies = fitness >= min_fitness && calm_ok;
    let reason = if !calm_ok {
        "rejected: buoy reports rough water".into()
    } else if fitness < min_fitness {
        format!("rejected: intent fitness {fitness:.2} < {min_fitness:.2}")
    } else {
        format!("qualified: fitness {fitness:.2}")
    };
    SceneVerdict {
        date: date.to_string(),
        sensor: sensor.to_string(),
        qualifies,
        fitness,
        wave_height_m: cond.wave_height_m,
        wind_speed_ms: cond.wind_speed_ms,
        cloud_pct: cond.cloud_pct,
        reason,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::env_conditions::{ScanIntent, SceneConditions};

    #[test]
    fn mode_parse() {
        assert_eq!(ScanMode::parse("recent"), Some(ScanMode::BeforeAfter));
        assert_eq!(ScanMode::parse("historical"), Some(ScanMode::Historical));
        assert_eq!(ScanMode::parse("xyz"), None);
    }

    #[test]
    fn before_after_brackets_event() {
        let plan = build_plan(
            ScanMode::BeforeAfter,
            45.87,
            -84.58,
            ScanIntent::DeepWreck,
            &["sentinel2"],
            Some("2022-11-15"),
            true,
        );
        // one pre_event + one post_event window for the single sensor
        assert!(plan.windows.iter().any(|w| w.label == "pre_event"));
        assert!(plan.windows.iter().any(|w| w.label == "post_event"));
        assert_eq!(plan.min_scenes, 20);
        assert_eq!(plan.target_scenes, 100);
    }

    #[test]
    fn historical_picks_years() {
        let plan = build_plan(
            ScanMode::Historical,
            45.87,
            -84.58,
            ScanIntent::DeepWreck,
            &["sentinel2"],
            None,
            true,
        );
        assert!(!plan.windows.is_empty(), "historical should pick year windows");
    }

    #[test]
    fn qualify_rejects_rough() {
        let cond = SceneConditions {
            wind_speed_ms: Some(2.0),
            wave_height_m: Some(0.1),
            cloud_pct: Some(3.0),
            days_since_storm: Some(5),
            turbidity: Some(1.0),
            month: 6,
        };
        let pass = qualify_scene("2024-06-10", "sentinel2", ScanIntent::Bathymetry, &cond, Some(true), 0.6);
        assert!(pass.qualifies);
        let fail = qualify_scene("2024-06-10", "sentinel2", ScanIntent::Bathymetry, &cond, Some(false), 0.6);
        assert!(!fail.qualifies, "rough buoy must reject");
    }
}
