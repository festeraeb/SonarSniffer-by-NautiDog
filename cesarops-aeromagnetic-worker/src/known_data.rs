//! Embedded known-wreck / ground-truth databases for the Lake Erie
//! discriminator cross-reference.
//!
//! Ported EXACTLY from `pipelines/mag/erie_wellhead_discriminator.py`:
//!   - `NIAGARA_DIVERS_WRECKS` (27 entries) — lines 166-194
//!   - `SHIPWRECKWORLD_WRECKS` (20 entries) — lines 196-217
//!   - `get_all_known_wrecks()` (NIAGARA + SHIPWRECKWORLD) — lines 220-222
//!   - `GROUND_TRUTH` label map — lines 229-233
//!   - `CONFIRMED_FIELD_SITES` — lines 236-246
//!   - `GT_GEO_RADIUS_M` — line 248
//!
//! The Python `KnownWreck(name, lat, lon, vessel_type, length_ft=0,
//! source="", hull_material="")` maps onto the Rust `KnownWreck` in
//! `discriminator.rs`, whose discriminator-facing fields are
//! `salvage_status` / `magnetic_potential` (both empty here — none of the
//! Python entries are flagged) and `depth_ft` (0 — unknown for all entries).
//! The Python `hull_material` value has no Rust field and is dropped.

use crate::discriminator::KnownWreck;

/// Compact constructor for an embedded known wreck.
///
/// Mirrors the Python `KnownWreck(name, lat, lon, vessel_type, length_ft,
/// source)` call shape. `depth_ft`, `salvage_status` and `magnetic_potential`
/// are always defaulted (0 / "" / "") to match the Python data, which leaves
/// them unset for every embedded entry.
fn kw(name: &str, lat: f64, lon: f64, vessel_type: &str, length_ft: f64, source: &str) -> KnownWreck {
    KnownWreck {
        name: name.to_string(),
        lat,
        lon,
        vessel_type: vessel_type.to_string(),
        length_ft,
        depth_ft: 0.0,
        source: source.to_string(),
        salvage_status: String::new(),
        magnetic_potential: String::new(),
    }
}

/// Niagara Divers Association mooring locations (Eastern Basin, Lake Erie).
///
/// Ports `NIAGARA_DIVERS_WRECKS` (erie_wellhead_discriminator.py lines 166-194).
/// Parsed from https://www.niagaradivers.com/moor/locations.html.
/// Entries without an explicit length default to 0 (Python `length_ft=0`).
pub fn niagara_divers_wrecks() -> Vec<KnownWreck> {
    vec![
        kw("Acme", 42.610017, -79.497367, "schooner-barge", 0.0, "NDA"),
        kw("Atlantic", 42.510333, -80.084767, "steamer", 0.0, "NDA"),
        kw("Boland", 42.379900, -79.731550, "steamer", 0.0, "NDA"),
        kw("Betty Hedger", 42.418500, -79.608800, "barge", 0.0, "NDA"),
        kw("Brunswick", 42.591833, -79.408783, "steamer", 0.0, "NDA"),
        kw("Carlingford", 42.653813, -79.476617, "schooner", 0.0, "NDA"),
        kw("CB Benson", 42.771033, -79.243483, "schooner", 0.0, "NDA"),
        kw("Cracker", 42.558083, -79.860817, "schooner", 0.0, "NDA"),
        kw("Dean Richmond", 42.290350, -79.930983, "propeller", 237.0, "NDA"),
        kw("Dupuis #10", 42.818250, -79.221667, "barge", 0.0, "NDA"),
        kw("Finch", 42.849417, -78.983817, "tug", 0.0, "NDA"),
        kw("George Finney", 42.668117, -79.604167, "schooner", 0.0, "NDA"),
        kw("Indiana", 42.296983, -79.998450, "propeller", 0.0, "NDA"),
        kw("Niagara", 42.738500, -79.604750, "steamer", 0.0, "NDA"),
        kw("O.W.Cheney", 42.837517, -79.007950, "schooner", 0.0, "NDA"),
        kw("Oneida/Arches", 42.457933, -80.017017, "steamer", 0.0, "NDA"),
        kw("Oxford", 42.480917, -79.863717, "schooner", 0.0, "NDA"),
        kw("Passaic", 42.479267, -79.463033, "steamer", 0.0, "NDA"),
        kw("Persian", 42.563017, -79.911600, "steamer", 0.0, "NDA"),
        kw("Raleigh", 42.865433, -79.154233, "steamer", 0.0, "NDA"),
        kw("Smith", 42.474767, -79.984350, "schooner", 0.0, "NDA"),
        kw("St. James", 42.450233, -80.122183, "schooner", 0.0, "NDA"),
        kw("Stern Castle", 42.504900, -80.039650, "unknown", 0.0, "NDA"),
        kw("Stonewreck", 42.667933, -79.396333, "unknown", 0.0, "NDA"),
        kw("Tonawanda", 42.839983, -78.982200, "steamer", 0.0, "NDA"),
        kw("Tradewind", 42.425267, -80.200933, "schooner", 0.0, "NDA"),
        kw("Washington Irving", 42.539517, -79.460600, "brig", 0.0, "NDA"),
    ]
}

/// ShipwreckWorld Lake Erie entries (researched positions).
///
/// Ports `SHIPWRECKWORLD_WRECKS` (erie_wellhead_discriminator.py lines 196-217).
/// The Python `hull_material` values (steel for Sand Merchant / Frank E. Vigor /
/// Colgate) have no Rust field and are dropped. The final entry, Colgate, is the
/// confirmed whaleback target #103.
pub fn shipwreckworld_wrecks() -> Vec<KnownWreck> {
    vec![
        kw("Craftsman", 42.160, -79.800, "barge", 90.0, "ShipwreckWorld"),
        kw("John Pridgeon", 42.370, -81.050, "steamer", 222.0, "ShipwreckWorld"),
        kw("Sand Merchant", 42.475, -79.870, "sandsucker", 252.0, "ShipwreckWorld"),
        kw("Two Fannies", 42.350, -80.100, "bark", 152.0, "ShipwreckWorld"),
        kw("Mecosta", 42.100, -81.600, "steamer", 281.0, "ShipwreckWorld"),
        kw("John B. Griffin", 42.450, -80.300, "tug", 57.0, "ShipwreckWorld"),
        kw("H.G. Cleveland", 42.300, -80.500, "schooner", 137.0, "ShipwreckWorld"),
        kw("Mabel Wilson", 42.200, -80.800, "schooner", 243.0, "ShipwreckWorld"),
        kw("Fannie L. Jones", 42.350, -80.600, "schooner", 93.0, "ShipwreckWorld"),
        kw("Charles H. Davis", 42.400, -80.200, "steamer", 145.0, "ShipwreckWorld"),
        kw("Algeria", 42.250, -80.700, "schooner-barge", 288.0, "ShipwreckWorld"),
        kw("Admiral", 41.800, -81.700, "tug", 93.0, "ShipwreckWorld"),
        kw("Dundee", 42.100, -80.900, "schooner-barge", 211.0, "ShipwreckWorld"),
        kw("Duke Luedtke", 41.500, -81.600, "tug", 69.0, "ShipwreckWorld"),
        kw("Steven F. Gale", 42.300, -79.800, "schooner", 123.0, "ShipwreckWorld"),
        kw("F.A. Meyer", 42.080, -81.500, "steamer", 256.0, "ShipwreckWorld"),
        kw("Valentine", 42.350, -80.300, "schooner", 128.0, "ShipwreckWorld"),
        kw("Frank E. Vigor", 42.150, -81.400, "freighter", 0.0, "ShipwreckWorld"),
        kw("Colonial", 42.500, -79.900, "steamer", 0.0, "ShipwreckWorld"),
        // Known whaleback wreck (confirmed ground-truth target #103).
        kw("Colgate", 42.173, -81.740, "whaleback", 308.0, "confirmed_target_103"),
    ]
}

/// Straits of Mackinac Shipwreck Preserve — dive-charted wrecks.
///
/// Coordinates from the Michigan Underwater Preserves registry
/// (michiganpreserves.org), the same dive-grade source divers use to locate
/// these sites. Built by `scripts/build_straits_ground_truth.py` from
/// `outputs/great_lakes_preserve_wrecks.json`. Depths are charted feet where
/// the registry lists them (0 = unlisted, not surface). These are the
/// ground-truth targets for validating Straits detection runs.
pub fn straits_wrecks() -> Vec<KnownWreck> {
    vec![
        kw("(formerly known as) St. Andrew", 45.70085, -84.52992, "unknown", 62.0, "MI_preserve"),
        kw("Albemarle", 45.71563, -84.56307, "unknown", 0.0, "MI_preserve"),
        kw("C.H. Johnson", 45.87543, -84.83573, "unknown", 0.0, "MI_preserve"),
        kw("Cayuga", 45.72065, -85.19002, "unknown", 0.0, "MI_preserve"),
        kw("Cedarville", 45.78725, -84.67080, "bulk_freighter", 40.0, "MI_preserve"),
        kw("Cedarville stern", 45.78870, -84.67207, "bulk_freighter", 0.0, "MI_preserve"),
        kw("Colonel Ellsworth", 45.81238, -85.01760, "schooner", 0.0, "MI_preserve"),
        kw("Dolphin", 45.81887, -84.99580, "unknown", 0.0, "MI_preserve"),
        kw("Eber Ward", 45.81272, -84.81888, "steamer", 111.0, "MI_preserve"),
        kw("Eber Ward bow", 45.81213, -84.81888, "steamer", 0.0, "MI_preserve"),
        kw("Elva", 45.87455, -84.59425, "unknown", 0.0, "MI_preserve"),
        kw("Fred McBrier", 45.80570, -84.92168, "steamer", 0.0, "MI_preserve"),
        kw("Genesee Chief", 45.66235, -84.43608, "schooner", 0.0, "MI_preserve"),
        kw("Henry Clay", 45.71403, -84.56495, "unknown", 0.0, "MI_preserve"),
        kw("L.B. Coates", 45.66417, -84.48367, "unknown", 0.0, "MI_preserve"),
        kw("Leviathan", 45.66083, -84.43250, "unknown", 0.0, "MI_preserve"),
        kw("Lucy J. Clark", 45.66590, -85.01698, "schooner", 0.0, "MI_preserve"),
        kw("M. Stalker", 45.79382, -84.68408, "schooner", 0.0, "MI_preserve"),
        kw("Maitland", 45.80417, -84.87578, "schooner", 0.0, "MI_preserve"),
        kw("Minneapolis", 45.80837, -84.73163, "steamer", 0.0, "MI_preserve"),
        kw("Newell Eddy", 45.78150, -84.23017, "schooner_barge", 165.0, "MI_preserve"),
        kw("Northwest", 45.79083, -84.85775, "schooner", 75.0, "MI_preserve"),
        kw("Sandusky", 45.79948, -84.83758, "brig", 0.0, "MI_preserve"),
        kw("Uganda", 45.84255, -85.04997, "steamer", 185.0, "MI_preserve"),
        kw("William H. Barnum", 45.74513, -84.63110, "steamer", 0.0, "MI_preserve"),
        kw("William Young", 45.81295, -84.69872, "schooner", 0.0, "MI_preserve"),
    ]
}

/// Combined list from all known wreck sources (NIAGARA + SHIPWRECKWORLD).
///
/// Ports `get_all_known_wrecks()` (erie_wellhead_discriminator.py lines 220-222),
/// extended with the Straits of Mackinac preserve set so detection runs in the
/// Straits AOI cross-reference against real dive-charted wrecks.
pub fn all_known_wrecks() -> Vec<KnownWreck> {
    let mut wrecks = niagara_divers_wrecks();
    wrecks.extend(shipwreckworld_wrecks());
    wrecks.extend(straits_wrecks());
    wrecks
}

// ── Ground truth labeling ───────────────────────────────────────────────────

/// A confirmed ground-truth label keyed by adaptive-scan `label_id`.
/// `class` is one of "wreck" | "wellhead"; `name` is the human-readable label.
#[derive(Debug, Clone, Copy)]
pub struct GroundTruthLabel {
    pub label_id: u32,
    pub class: &'static str,
    pub name: &'static str,
}

/// Confirmed ground truth from field work (used when adaptive-scan `label_id`s
/// align). Ports the `GROUND_TRUTH` dict (erie_wellhead_discriminator.py lines
/// 229-233).
pub const GROUND_TRUTH: [GroundTruthLabel; 3] = [
    GroundTruthLabel { label_id: 103, class: "wreck", name: "Colgate (whaleback)" },
    GroundTruthLabel { label_id: 63, class: "wellhead", name: "Gas wellhead" },
    GroundTruthLabel { label_id: 85, class: "wellhead", name: "Gas wellhead" },
];

/// Look up a confirmed ground-truth label by adaptive-scan `label_id`.
/// Returns `(class, name)` when present. Ports `label_id in GROUND_TRUTH`.
pub fn ground_truth_for_label(label_id: u32) -> Option<(&'static str, &'static str)> {
    GROUND_TRUTH
        .iter()
        .find(|g| g.label_id == label_id)
        .map(|g| (g.class, g.name))
}

/// A dive-confirmed / field-GPS site used for geographic ground-truth
/// validation (not keyed by `label_id`).
#[derive(Debug, Clone, Copy)]
pub struct ConfirmedFieldSite {
    pub name: &'static str,
    pub lat: f64,
    pub lon: f64,
    pub class: &'static str,
    pub radius_m: f64,
    pub source: &'static str,
}

/// Dive-confirmed field sites for geographic validation.
/// Ports `CONFIRMED_FIELD_SITES` (erie_wellhead_discriminator.py lines 236-246).
pub const CONFIRMED_FIELD_SITES: [ConfirmedFieldSite; 1] = [ConfirmedFieldSite {
    name: "Colgate (#103)",
    lat: 42.173,
    lon: -81.740,
    class: "wreck",
    radius_m: 1200.0,
    source: "field_confirmed",
}];

/// Default geographic ground-truth match radius (m) used when a confirmed field
/// site does not specify its own. Ports `GT_GEO_RADIUS_M`
/// (erie_wellhead_discriminator.py line 248).
pub const GT_GEO_RADIUS_M: f64 = 1500.0;

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Wreck-source counts: 27 NDA + 20 ShipwreckWorld (Lake Erie) + 26 Straits
    /// of Mackinac preserve = 73 combined.
    #[test]
    fn test_all_known_wrecks_count() {
        assert_eq!(niagara_divers_wrecks().len(), 27, "NDA wreck count");
        assert_eq!(shipwreckworld_wrecks().len(), 20, "ShipwreckWorld wreck count");
        assert_eq!(straits_wrecks().len(), 26, "Straits preserve wreck count");
        assert_eq!(all_known_wrecks().len(), 73, "combined wreck count");
    }

    /// Every embedded wreck has a plausible Great Lakes coordinate and a
    /// non-empty name/source. Lake Erie wrecks sit ~41-43 N / -84..-78 W;
    /// the Straits of Mackinac set sits ~45.6-46.0 N / -85.3..-84.2 W.
    #[test]
    fn test_all_known_wrecks_valid_coords() {
        for w in all_known_wrecks() {
            let in_erie = (41.0..=43.0).contains(&w.lat) && (-84.0..=-78.0).contains(&w.lon);
            let in_straits = (45.5..=46.1).contains(&w.lat) && (-85.3..=-84.2).contains(&w.lon);
            assert!(
                in_erie || in_straits,
                "wreck '{}' ({}, {}) outside both Lake Erie and Straits bands",
                w.name,
                w.lat,
                w.lon
            );
            assert!(!w.name.is_empty(), "wreck name must be non-empty");
            assert!(!w.source.is_empty(), "wreck source must be non-empty");
            // No embedded entry is flagged for the disposition filter.
            assert!(w.salvage_status.is_empty(), "salvage_status must be empty");
            assert!(w.magnetic_potential.is_empty(), "magnetic_potential must be empty");
        }
    }

    /// The confirmed Colgate whaleback (#103) is present at its field-GPS coord.
    #[test]
    fn test_colgate_present() {
        let wrecks = all_known_wrecks();
        let colgate = wrecks
            .iter()
            .find(|w| w.name == "Colgate")
            .expect("Colgate must be embedded");
        assert_eq!(colgate.vessel_type, "whaleback");
        assert!((colgate.lat - 42.173).abs() < 1e-6);
        assert!((colgate.lon - (-81.740)).abs() < 1e-6);
    }

    /// Ground-truth label lookups match the Python `GROUND_TRUTH` dict.
    #[test]
    fn test_ground_truth_lookup() {
        assert_eq!(ground_truth_for_label(103), Some(("wreck", "Colgate (whaleback)")));
        assert_eq!(ground_truth_for_label(63), Some(("wellhead", "Gas wellhead")));
        assert_eq!(ground_truth_for_label(85), Some(("wellhead", "Gas wellhead")));
        assert_eq!(ground_truth_for_label(999), None);
    }

    /// Cross-reference activation: a candidate placed on a known wreck coordinate
    /// must match against the embedded wreck database and have its `ground_truth`
    /// set to "wreck" with the correct nearest-wreck name. This is the wiring the
    /// task adds — previously `wrecks` was always empty so this never fired.
    #[test]
    fn test_cross_reference_activates_with_embedded_wrecks() {
        use crate::discriminator::{cross_reference_candidate, CandidateMatch};

        let wrecks = all_known_wrecks();
        // Place the candidate exactly on the confirmed Colgate (#103) coordinate.
        let mut cand = CandidateMatch {
            label_id: 1,
            center_lat: 42.173,
            center_lon: -81.740,
            dipole_score: 0.0,
            dipole_verdict: String::new(),
            ground_truth: String::new(),
            ground_truth_name: String::new(),
            well_distance_m: None,
            nearest_wellhead: None,
            wreck_distance_m: None,
            nearest_known_wreck: None,
            nearest_wreck_salvage_status: String::new(),
            nearest_wreck_magnetic_potential: String::new(),
            bonus_score: 0.0,
            curvelet_energy_ratio: None,
        };

        // No wells, default Python radii (wellhead 2000 m, wreck 5000 m), Loran
        // correction on (the warp is < 5 km so the wreck still matches).
        cross_reference_candidate(&mut cand, &[], &wrecks, 2000.0, 5000.0, true);

        assert_eq!(cand.ground_truth, "wreck", "cross-reference must activate");
        assert_eq!(cand.nearest_known_wreck.as_deref(), Some("Colgate"));
        assert!(
            cand.wreck_distance_m.map(|d| d < 5000.0).unwrap_or(false),
            "matched wreck must be within the wreck radius"
        );
        // Proximity boost was applied (bonus_score > 0).
        assert!(cand.bonus_score > 0.0, "near-wreck proximity boost must apply");
    }

}
