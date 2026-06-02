//! Mission-intent-driven year/window selection for Great Lakes scene pulls.
//!
//! Low water is ONE strategy, not THE strategy. The best years depend on what
//! you're hunting:
//!   - cold-sink / optical wreck hunt → low-water years (wreck nearer surface)
//!   - recent sinking (Charley Brown, Rosa) → most-recent years (not there before)
//!   - zebra/quagga clarity → years AFTER the low/invasion settled (clarity
//!     improves later, not in the low year itself)
//!   - hydrocarbon spill → a specific event window, near-real-time
//!
//! So selection is keyed to a [`ScanIntent`], and year ranking is clamped to the
//! sensor's archive (Sentinel-2 2017+ cannot reach the 2012-2013 record lows —
//! those need Landsat).
//!
//! Water-level ranking is from NOAA GLERL / USACE Great Lakes monthly mean
//! levels (public record); years listed lowest-first.

use chrono::{Datelike, Local};

// ── Sensor archives ───────────────────────────────────────────────────────────

pub const SENTINEL2_ARCHIVE_START: i32 = 2017; // full S2A+S2B; S2A alone 2015
pub const LANDSAT_ARCHIVE_START: i32 = 1984; // Landsat 5 TM onward

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SensorFamily {
    Sentinel2,
    Landsat,
}

impl SensorFamily {
    pub fn from_str(s: &str) -> Self {
        if s.to_lowercase().contains("landsat") {
            SensorFamily::Landsat
        } else {
            SensorFamily::Sentinel2
        }
    }
    pub fn archive_start(self) -> i32 {
        match self {
            SensorFamily::Sentinel2 => SENTINEL2_ARCHIVE_START,
            SensorFamily::Landsat => LANDSAT_ARCHIVE_START,
        }
    }
}

// ── Scan intent ───────────────────────────────────────────────────────────────

/// What the operator is hunting — drives which years/windows are "best".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScanIntent {
    /// Submerged-wreck cold-sink / optical: low water raises the target.
    LowWaterWreck,
    /// A wreck that sank recently — only the most recent years can show it.
    RecentSinking,
    /// Zebra/quagga clarity anomaly: clearer in the years AFTER the lows.
    ZebraClarity,
    /// Time-critical event (hydrocarbon spill, SAR): most-recent window.
    EventResponse,
    /// No preference — chronological recent-first.
    Generic,
}

impl ScanIntent {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().replace([' ', '-'], "_").as_str() {
            "low_water" | "low_water_wreck" | "wreck" | "cold_sink" => ScanIntent::LowWaterWreck,
            "recent" | "recent_sinking" | "new_wreck" => ScanIntent::RecentSinking,
            "zebra" | "zebra_clarity" | "clarity" | "mussel" => ScanIntent::ZebraClarity,
            "event" | "event_response" | "spill" | "hydrocarbon" | "sar" => {
                ScanIntent::EventResponse
            }
            _ => ScanIntent::Generic,
        }
    }
}

// ── Basin routing (ordered, non-overlapping; first match wins) ─────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LakeBasin {
    MichiganHuron,
    Superior,
    Erie,
    Ontario,
    Unknown,
}

/// Route an AOI centre to a basin. Checks are ordered most-specific first so
/// the boxes don't overlap (Straits ~45.8N/-84.75W must land in Michigan-Huron,
/// not Superior). Superior is gated to its true northern/western extent.
pub fn basin_for(lat: f64, lon: f64) -> LakeBasin {
    // Erie: south-east, shallow. Tight box, checked first.
    if (41.2..=43.0).contains(&lat) && (-83.6..=-78.7).contains(&lon) {
        return LakeBasin::Erie;
    }
    // Ontario: east of Erie.
    if (43.1..=44.4).contains(&lat) && (-79.9..=-75.8).contains(&lon) {
        return LakeBasin::Ontario;
    }
    // Superior: only the true northern basin (lat >= 46.5) OR far west (lon <= -86)
    // so the Straits (45.8N/-84.75W) does NOT fall through to Superior.
    if (46.5..=49.5).contains(&lat) && (-92.5..=-84.3).contains(&lon) {
        return LakeBasin::Superior;
    }
    // Michigan-Huron: everything else in the central Great Lakes box
    // (Lake Michigan south of the Straits + Lake Huron east, shared datum).
    if (41.6..=46.5).contains(&lat) && (-88.5..=-82.0).contains(&lon) {
        return LakeBasin::MichiganHuron;
    }
    LakeBasin::Unknown
}

/// Historic low-water years for a basin, lowest-first (NOAA/USACE monthly means).
fn ranked_low_years(basin: LakeBasin) -> &'static [i32] {
    match basin {
        LakeBasin::MichiganHuron => &[2013, 2012, 2007, 2003, 1964, 1965, 2001, 2000, 2014, 2010],
        LakeBasin::Superior => &[2007, 2010, 2013, 2012, 1926, 2003, 2000, 2014],
        LakeBasin::Erie => &[1934, 1936, 1964, 2012, 2001, 1965, 2007],
        LakeBasin::Ontario => &[1934, 1935, 1965, 2007, 2012],
        LakeBasin::Unknown => &[],
    }
}

/// Years of notably HIGH/recovered clarity-relevant water for the zebra/quagga
/// strategy — the post-invasion, post-low recovery period when mussel filtering
/// makes the column clearest. Most-recent-first within the sensor archive.
fn clarity_years(this_year: i32, start: i32, n: usize) -> Vec<i32> {
    // Clarity keeps improving; just take the most recent complete years the
    // sensor can serve (post-2017 for S2 is squarely in the high-clarity era).
    let mut out = Vec::new();
    let mut y = this_year - 1;
    while out.len() < n && y >= start {
        out.push(y);
        y -= 1;
    }
    out
}

// ── Public selection ───────────────────────────────────────────────────────────

/// Select up to `n` priority years for the AOI given the scan intent, clamped to
/// the sensor archive and excluding the current (incomplete) year.
pub fn select_years(
    lat: f64,
    lon: f64,
    sensor: SensorFamily,
    intent: ScanIntent,
    n: usize,
) -> Vec<i32> {
    let start = sensor.archive_start();
    let this_year = Local::now().year();

    match intent {
        ScanIntent::LowWaterWreck => {
            let basin = basin_for(lat, lon);
            ranked_low_years(basin)
                .iter()
                .copied()
                .filter(|&y| y >= start && y < this_year)
                .take(n)
                .collect()
        }
        // Recent sinking + event response + generic all want most-recent-first;
        // event response would additionally narrow the window at the caller.
        ScanIntent::RecentSinking | ScanIntent::EventResponse | ScanIntent::Generic => {
            let mut out = Vec::new();
            let mut y = this_year; // include current year for fresh events
            while out.len() < n && y >= start {
                out.push(y);
                y -= 1;
            }
            out
        }
        ScanIntent::ZebraClarity => clarity_years(this_year, start, n),
    }
}

/// Diagnostic: does the requested sensor reach this basin's record-low year?
/// If false (e.g. Sentinel-2 over Michigan-Huron, whose low is 2013), the caller
/// should switch to Landsat to reach the true lows.
pub fn sensor_reaches_record_low(lat: f64, lon: f64, sensor: SensorFamily) -> bool {
    match ranked_low_years(basin_for(lat, lon)).first() {
        Some(&record_low) => record_low >= sensor.archive_start(),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn straits_routes_to_michigan_huron() {
        assert_eq!(basin_for(45.82, -84.75), LakeBasin::MichiganHuron);
    }

    #[test]
    fn basin_routing_is_unambiguous() {
        assert_eq!(basin_for(41.9, -81.0), LakeBasin::Erie);
        assert_eq!(basin_for(43.6, -77.0), LakeBasin::Ontario);
        assert_eq!(basin_for(47.5, -87.5), LakeBasin::Superior);
        assert_eq!(basin_for(43.5, -86.5), LakeBasin::MichiganHuron); // S. Lake Michigan
    }

    #[test]
    fn low_water_sentinel2_clamped_no_record_low() {
        let years = select_years(45.82, -84.75, SensorFamily::Sentinel2, ScanIntent::LowWaterWreck, 4);
        assert!(years.iter().all(|&y| y >= SENTINEL2_ARCHIVE_START));
        assert!(!years.contains(&2013) && !years.contains(&2012));
        assert!(!sensor_reaches_record_low(45.82, -84.75, SensorFamily::Sentinel2));
    }

    #[test]
    fn low_water_landsat_includes_true_lows() {
        let years = select_years(45.82, -84.75, SensorFamily::Landsat, ScanIntent::LowWaterWreck, 4);
        assert_eq!(years.first(), Some(&2013));
        assert!(years.contains(&2012));
        assert!(sensor_reaches_record_low(45.82, -84.75, SensorFamily::Landsat));
    }

    #[test]
    fn recent_sinking_is_most_recent_first() {
        // Charley Brown / Rosa case: the wreck only exists in recent years.
        let years = select_years(45.82, -84.75, SensorFamily::Sentinel2, ScanIntent::RecentSinking, 3);
        let this_year = Local::now().year();
        assert_eq!(years.first(), Some(&this_year));
        // Strictly descending.
        for w in years.windows(2) {
            assert!(w[0] > w[1]);
        }
    }

    #[test]
    fn zebra_clarity_avoids_low_years_takes_recent() {
        // Clarity improves AFTER the lows — selection should be recent, not 2013.
        let years = select_years(45.82, -84.75, SensorFamily::Sentinel2, ScanIntent::ZebraClarity, 4);
        assert!(years.iter().all(|&y| y >= SENTINEL2_ARCHIVE_START));
        assert!(!years.contains(&2013));
        // Most-recent-first, excluding the incomplete current year.
        assert_eq!(years.first(), Some(&(Local::now().year() - 1)));
    }

    #[test]
    fn intent_parsing() {
        assert_eq!(ScanIntent::from_str("recent sinking"), ScanIntent::RecentSinking);
        assert_eq!(ScanIntent::from_str("zebra-clarity"), ScanIntent::ZebraClarity);
        assert_eq!(ScanIntent::from_str("hydrocarbon"), ScanIntent::EventResponse);
        assert_eq!(ScanIntent::from_str("low_water"), ScanIntent::LowWaterWreck);
        assert_eq!(ScanIntent::from_str("whatever"), ScanIntent::Generic);
    }
}
