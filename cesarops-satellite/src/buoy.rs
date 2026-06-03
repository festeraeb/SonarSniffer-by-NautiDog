//! Great Lakes buoy wave/wind gate for scene selection (NDBC + GLOS).
//!
//! The thermal cold/heat-sink and clarity-plume signals require CALM water:
//! wind-driven waves churn the surface and destroy the subtle column signal a
//! deep wreck produces. This module looks up the nearest Great Lakes buoy to an
//! AOI and reports the measured wave height + wind speed on a scene's
//! acquisition date, so the pipeline can REJECT rough-water scenes before they
//! enter the stack.
//!
//! Data source: NDBC historical standard-meteorological files
//!   https://www.ndbc.noaa.gov/view_text_file.php?filename=<ID>h<YYYY>.txt.gz&dir=data/historical/stdmet/
//! Columns: #YY MM DD hh mm WDIR WSPD GST WVHT DPD APD MWD PRES ATMP WTMP DEWP VIS TIDE
//!   WVHT = significant wave height (m), WSPD = wind speed (m/s); 99.0 = missing.
//!
//! GLOS buoys (e.g. McGulpin Point, Mackinac Straits West) also report via NDBC
//! station IDs, so the same fetch path covers them.

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

/// A Great Lakes buoy station with position. Coverage spans all five lakes so
/// any AOI in the basin has a nearby station.
#[derive(Debug, Clone, Copy)]
pub struct BuoyStation {
    pub id: &'static str,
    pub lat: f64,
    pub lon: f64,
    pub lake: &'static str,
    pub name: &'static str,
}

/// Curated Great Lakes NDBC/GLOS station registry. Not exhaustive, but spread
/// across all five lakes so `nearest_station` always finds one within ~100 km.
pub const GREAT_LAKES_BUOYS: &[BuoyStation] = &[
    // ── Lake Michigan ──
    BuoyStation { id: "45007", lat: 42.674, lon: -87.026, lake: "Lake Michigan", name: "S Lake Michigan" },
    BuoyStation { id: "45002", lat: 45.344, lon: -86.411, lake: "Lake Michigan", name: "N Lake Michigan" },
    BuoyStation { id: "45013", lat: 42.736, lon: -87.190, lake: "Lake Michigan", name: "Milwaukee" },
    BuoyStation { id: "45014", lat: 44.794, lon: -87.758, lake: "Lake Michigan", name: "Sturgeon Bay" },
    BuoyStation { id: "45210", lat: 44.282, lon: -87.566, lake: "Lake Michigan", name: "Rawley Point" },
    // ── Straits of Mackinac (the AOI) ──
    BuoyStation { id: "45175", lat: 45.825, lon: -84.772, lake: "Lake Huron", name: "Mackinac Straits West" },
    // ── Lake Huron ──
    BuoyStation { id: "45008", lat: 44.283, lon: -82.416, lake: "Lake Huron", name: "S Lake Huron" },
    BuoyStation { id: "45003", lat: 45.351, lon: -82.840, lake: "Lake Huron", name: "N Lake Huron" },
    BuoyStation { id: "45162", lat: 44.988, lon: -83.269, lake: "Lake Huron", name: "Thunder Bay / Alpena" },
    BuoyStation { id: "45212", lat: 45.351, lon: -82.840, lake: "Lake Huron", name: "Southampton" },
    // ── Lake Superior ──
    BuoyStation { id: "45001", lat: 47.582, lon: -87.394, lake: "Lake Superior", name: "Mid Superior" },
    BuoyStation { id: "45004", lat: 47.585, lon: -86.585, lake: "Lake Superior", name: "E Superior" },
    BuoyStation { id: "45006", lat: 47.337, lon: -89.787, lake: "Lake Superior", name: "W Superior" },
    BuoyStation { id: "45027", lat: 46.858, lon: -91.793, lake: "Lake Superior", name: "McQuade Harbor" },
    // ── Lake Erie ──
    BuoyStation { id: "45005", lat: 41.677, lon: -82.398, lake: "Lake Erie", name: "W Lake Erie" },
    BuoyStation { id: "45132", lat: 42.460, lon: -81.220, lake: "Lake Erie", name: "Port Stanley" },
    BuoyStation { id: "45165", lat: 41.704, lon: -83.264, lake: "Lake Erie", name: "Toledo" },
    // ── Lake Ontario ──
    BuoyStation { id: "45012", lat: 43.619, lon: -77.405, lake: "Lake Ontario", name: "E Lake Ontario" },
    BuoyStation { id: "45159", lat: 43.770, lon: -78.980, lake: "Lake Ontario", name: "Toronto" },
    BuoyStation { id: "45139", lat: 43.787, lon: -76.870, lake: "Lake Ontario", name: "Mexico Bay" },
];

/// Wave/wind condition at a buoy for a given date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuoyCondition {
    pub station_id: String,
    pub station_name: String,
    pub distance_km: f64,
    pub date: String,
    /// Mean significant wave height (m) over the day's records.
    pub wave_height_m: Option<f64>,
    /// Mean wind speed (m/s) over the day's records.
    pub wind_speed_ms: Option<f64>,
    /// True if conditions pass the calm gate (low wave + low wind).
    pub is_calm: bool,
}

/// Calm thresholds. A scene is "calm enough" for the thermal/plume stack when
/// wave height and wind are both below these. Tunable via Knobs later.
pub const CALM_WAVE_HEIGHT_M: f64 = 0.30; // 30 cm — near-glassy
pub const CALM_WIND_SPEED_MS: f64 = 5.0; // ~10 kt

fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0_f64;
    let (p1, p2) = (lat1.to_radians(), lat2.to_radians());
    let dp = (lat2 - lat1).to_radians();
    let dl = (lon2 - lon1).to_radians();
    let a = (dp / 2.0).sin().powi(2) + p1.cos() * p2.cos() * (dl / 2.0).sin().powi(2);
    2.0 * r * a.sqrt().asin()
}

/// Nearest Great Lakes buoy to a point.
pub fn nearest_station(lat: f64, lon: f64) -> (&'static BuoyStation, f64) {
    let mut best = &GREAT_LAKES_BUOYS[0];
    let mut best_d = f64::INFINITY;
    for s in GREAT_LAKES_BUOYS {
        let d = haversine_km(lat, lon, s.lat, s.lon);
        if d < best_d {
            best_d = d;
            best = s;
        }
    }
    (best, best_d)
}

/// Fetch + parse NDBC historical stdmet for a station/year, returning the mean
/// WVHT and WSPD for the requested date. Async (uses the shared reqwest client).
/// Returns None for both if no usable records that day.
pub async fn fetch_day_condition(
    client: &reqwest::Client,
    station_id: &str,
    date: NaiveDate,
) -> (Option<f64>, Option<f64>) {
    let year = date.format("%Y").to_string();
    let url = format!(
        "https://www.ndbc.noaa.gov/view_text_file.php?filename={station_id}h{year}.txt.gz&dir=data/historical/stdmet/"
    );
    let body = match client.get(&url).send().await {
        Ok(r) if r.status().is_success() => match r.text().await {
            Ok(t) => t,
            Err(_) => return (None, None),
        },
        _ => return (None, None),
    };
    parse_day_wvht_wspd(&body, date)
}

/// Parse the NDBC stdmet text, averaging WVHT (col 8) and WSPD (col 6) over all
/// records matching `date`. 99.0 / 99.00 = missing → skipped.
fn parse_day_wvht_wspd(body: &str, date: NaiveDate) -> (Option<f64>, Option<f64>) {
    use chrono::Datelike;
    let y = date.format("%Y").to_string();
    let mut wvht_sum = 0.0;
    let mut wvht_n = 0;
    let mut wspd_sum = 0.0;
    let mut wspd_n = 0;
    for line in body.lines() {
        if line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        // #YY MM DD hh mm WDIR WSPD GST WVHT ...
        if cols[0] != y || cols[1].parse::<u32>().ok() != Some(date.month())
            || cols[2].parse::<u32>().ok() != Some(date.day())
        {
            continue;
        }
        if let Ok(wspd) = cols[6].parse::<f64>() {
            if wspd < 90.0 {
                wspd_sum += wspd;
                wspd_n += 1;
            }
        }
        if let Ok(wvht) = cols[8].parse::<f64>() {
            if wvht < 90.0 {
                wvht_sum += wvht;
                wvht_n += 1;
            }
        }
    }
    let wvht = if wvht_n > 0 { Some(wvht_sum / wvht_n as f64) } else { None };
    let wspd = if wspd_n > 0 { Some(wspd_sum / wspd_n as f64) } else { None };
    (wvht, wspd)
}

/// Full calm-gate check for an AOI centre + scene date.
pub async fn condition_for(
    client: &reqwest::Client,
    lat: f64,
    lon: f64,
    date: NaiveDate,
) -> BuoyCondition {
    let (station, dist) = nearest_station(lat, lon);
    let (wvht, wspd) = fetch_day_condition(client, station.id, date).await;
    let is_calm = wvht.map_or(false, |w| w <= CALM_WAVE_HEIGHT_M)
        && wspd.map_or(true, |s| s <= CALM_WIND_SPEED_MS);
    BuoyCondition {
        station_id: station.id.to_string(),
        station_name: station.name.to_string(),
        distance_km: dist,
        date: date.format("%Y-%m-%d").to_string(),
        wave_height_m: wvht,
        wind_speed_ms: wspd,
        is_calm,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nearest_station_finds_straits() {
        // Burns target near Mackinac → nearest should be 45175 (Mackinac Straits West)
        let (s, d) = nearest_station(45.871, -84.586);
        assert_eq!(s.id, "45175");
        assert!(d < 30.0, "Straits buoy should be <30km, got {d}");
    }

    #[test]
    fn nearest_station_covers_all_lakes() {
        // A point in each lake resolves to a station in (or adjacent to) that lake.
        let erie = nearest_station(41.9, -81.7).0;
        assert!(erie.lake == "Lake Erie" || erie.lake == "Lake Ontario");
        let superior = nearest_station(47.5, -88.0).0;
        assert_eq!(superior.lake, "Lake Superior");
    }

    #[test]
    fn parse_calm_day() {
        let body = "#YY MM DD hh mm WDIR WSPD GST WVHT DPD APD MWD PRES ATMP WTMP DEWP VIS TIDE\n\
                    #yr mo dy hr mn degT m/s m/s m sec sec degT hPa degC degC degC mi ft\n\
                    2024 08 11 12 00 270 3.0 4.0 0.15 2.0 1.5 270 1010 20.0 18.0 15.0 99.0 99.0\n\
                    2024 08 11 13 00 270 3.5 4.5 0.20 2.0 1.5 270 1010 20.0 18.0 15.0 99.0 99.0\n";
        let date = NaiveDate::from_ymd_opt(2024, 8, 11).unwrap();
        let (wvht, wspd) = parse_day_wvht_wspd(body, date);
        assert!((wvht.unwrap() - 0.175).abs() < 1e-6);
        assert!((wspd.unwrap() - 3.25).abs() < 1e-6);
    }

    #[test]
    fn parse_skips_missing_99() {
        let body = "2024 08 11 12 00 270 99.0 99.0 99.00 99.0 99.0 999 9999 99.0 99.0 99.0 99.0 99.0\n";
        let date = NaiveDate::from_ymd_opt(2024, 8, 11).unwrap();
        let (wvht, wspd) = parse_day_wvht_wspd(body, date);
        assert!(wvht.is_none());
        assert!(wspd.is_none());
    }
}
