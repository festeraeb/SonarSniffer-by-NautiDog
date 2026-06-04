//! Open-Meteo historical-weather gate for scene selection.
//!
//! Populates the parts of [`crate::env_conditions::SceneConditions`] that the
//! buoy gate can't: cloud cover, days-since-storm, and the spring-runoff /
//! post-storm classification used by `download_priority` and `scene_score`.
//!
//! Data source: Open-Meteo Archive API (free, no auth):
//!   https://archive-api.open-meteo.com/v1/archive
//!     ?latitude=..&longitude=..&start_date=..&end_date=..
//!     &daily=wind_speed_10m_max,precipitation_sum,cloud_cover_mean
//!     &timezone=UTC&windspeed_unit=ms
//!
//! We fetch a window ending on the scene date so "days since the last storm"
//! can be computed from real history rather than guessed.

use crate::env_conditions::{
    classify_day, tag_conditions, DayCondition, WeatherDay, WeatherThresholds,
};
use chrono::{Duration, NaiveDate};
use serde::Deserialize;

const ARCHIVE_URL: &str = "https://archive-api.open-meteo.com/v1/archive";

/// How many days of history to pull before the scene date when computing
/// days-since-storm (covers the post-storm settling window with margin).
pub const LOOKBACK_DAYS: i64 = 10;

#[derive(Debug, Deserialize)]
struct ArchiveResponse {
    daily: Option<DailyBlock>,
}

#[derive(Debug, Deserialize)]
struct DailyBlock {
    time: Vec<String>,
    #[serde(default)]
    wind_speed_10m_max: Vec<Option<f64>>,
    #[serde(default)]
    precipitation_sum: Vec<Option<f64>>,
    #[serde(default)]
    cloud_cover_mean: Vec<Option<f64>>,
}

/// Result of the weather gate for one scene date.
#[derive(Debug, Clone)]
pub struct WeatherContext {
    pub date: NaiveDate,
    pub condition: DayCondition,
    /// Days since the most recent storm in the lookback window (None = no storm
    /// seen, or no data).
    pub days_since_storm: Option<i32>,
    /// Daily-mean cloud cover (%) on the scene date, if available.
    pub cloud_pct: Option<f64>,
    pub wind_speed_ms: Option<f64>,
    pub precip_mm: Option<f64>,
    pub data_available: bool,
}

impl WeatherContext {
    /// Empty/unknown context (used when the fetch fails — neutral, not penalising).
    pub fn unknown(date: NaiveDate) -> Self {
        Self {
            date,
            condition: DayCondition::Transitional,
            days_since_storm: None,
            cloud_pct: None,
            wind_speed_ms: None,
            precip_mm: None,
            data_available: false,
        }
    }
}

/// Fetch the Open-Meteo daily archive for `[date - LOOKBACK_DAYS, date]` at a
/// point and build a [`WeatherContext`] for `date`.
pub async fn weather_context(
    client: &reqwest::Client,
    lat: f64,
    lon: f64,
    date: NaiveDate,
) -> WeatherContext {
    let start = date - Duration::days(LOOKBACK_DAYS);
    let url = format!(
        "{ARCHIVE_URL}?latitude={lat:.4}&longitude={lon:.4}&start_date={start}&end_date={date}\
         &daily=wind_speed_10m_max,precipitation_sum,cloud_cover_mean&timezone=UTC&windspeed_unit=ms"
    );
    let resp = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => r,
        _ => return WeatherContext::unknown(date),
    };
    let parsed: ArchiveResponse = match resp.json().await {
        Ok(p) => p,
        Err(_) => return WeatherContext::unknown(date),
    };
    let daily = match parsed.daily {
        Some(d) if !d.time.is_empty() => d,
        _ => return WeatherContext::unknown(date),
    };

    // Build the WeatherDay series.
    let mut days: Vec<WeatherDay> = Vec::with_capacity(daily.time.len());
    for (i, t) in daily.time.iter().enumerate() {
        let d = match NaiveDate::parse_from_str(t, "%Y-%m-%d") {
            Ok(d) => d,
            Err(_) => continue,
        };
        let wind = daily.wind_speed_10m_max.get(i).copied().flatten().unwrap_or(0.0);
        let precip = daily.precipitation_sum.get(i).copied().flatten().unwrap_or(0.0);
        let cloud = daily.cloud_cover_mean.get(i).copied().flatten().unwrap_or(0.0);
        days.push(WeatherDay { date: d, wind_speed: wind, precip_mm: precip, cloud_cover: cloud, snowmelt_mm: 0.0 });
    }
    if days.is_empty() {
        return WeatherContext::unknown(date);
    }

    let thresh = WeatherThresholds::default();
    let tags = tag_conditions(&days, &thresh);

    // Days since the most recent storm at or before the scene date.
    let mut days_since_storm: Option<i32> = None;
    for (i, d) in days.iter().enumerate() {
        if d.date <= date && tags[i] == DayCondition::Storm {
            days_since_storm = Some((date - d.date).num_days() as i32);
        }
    }

    // Scene-date row (or the last available day as fallback).
    let scene_idx = days.iter().position(|d| d.date == date).unwrap_or(days.len() - 1);
    let scene_day = &days[scene_idx];
    let condition = classify_day(scene_day, &thresh);

    WeatherContext {
        date,
        condition,
        days_since_storm,
        cloud_pct: Some(scene_day.cloud_cover),
        wind_speed_ms: Some(scene_day.wind_speed),
        precip_mm: Some(scene_day.precip_mm),
        data_available: true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_is_neutral() {
        let w = WeatherContext::unknown(NaiveDate::from_ymd_opt(2023, 10, 12).unwrap());
        assert!(!w.data_available);
        assert!(w.days_since_storm.is_none());
        assert!(w.cloud_pct.is_none());
    }
}
