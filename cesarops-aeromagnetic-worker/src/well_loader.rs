//! OGSr petroleum-well CSV loader for the Lake Erie discriminator.
//!
//! Ports `load_ogsr_wells` and `LAKE_ERIE_BBOX` from
//! `pipelines/mag/erie_wellhead_discriminator.py` (bbox lines 100-103,
//! loader lines 107-160).
//!
//! The Python loader opens the OGSr export with `encoding="cp1252",
//! errors="replace"`, so we replicate that exactly: the raw file bytes are
//! decoded through a Windows-1252 table (undefined bytes → U+FFFD) before the
//! CSV is parsed. Rows are filtered to the Lake Erie region — either the
//! township literally contains "lake erie" (offshore wells) or the surface
//! coordinate falls inside `LAKE_ERIE_BBOX` (coastal wells whose magnetic
//! signature can show up in the aeromag grid).

use crate::discriminator::Wellhead;
use std::path::Path;

/// Lake Erie filtering bounding box.
/// Ports `LAKE_ERIE_BBOX` (erie_wellhead_discriminator.py lines 100-103).
pub struct LakeErieBbox {
    pub lat_min: f64,
    pub lat_max: f64,
    pub lon_min: f64,
    pub lon_max: f64,
}

pub const LAKE_ERIE_BBOX: LakeErieBbox = LakeErieBbox {
    lat_min: 41.35,
    lat_max: 42.90,
    lon_min: -83.50,
    lon_max: -78.80,
};

/// Windows-1252 high-range overrides for bytes 0x80-0x9F (the only range where
/// cp1252 differs from ISO-8859-1). `'\u{FFFD}'` marks the bytes that are
/// undefined in cp1252, matching Python's `errors="replace"`.
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// Decode raw bytes as Windows-1252 (cp1252) with undefined bytes replaced by
/// U+FFFD, matching Python `open(..., encoding="cp1252", errors="replace")`.
fn decode_cp1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
            // 0x00-0x7F and 0xA0-0xFF map 1:1 to the same Unicode code point.
            other => other as char,
        })
        .collect()
}

/// Load Ontario petroleum wells from an OGSr CSV export.
///
/// Ports `load_ogsr_wells` (erie_wellhead_discriminator.py lines 107-160).
///
/// Columns read (with the same fallbacks as the Python loader):
///   SUR_LAT83, SUR_LONG83, WELL_ID, FULL_NAME|WELL_NAME, CUR_STATUS,
///   WELL_TYPE|CLASS, TOWNSHIP, COUNTY, TARGET.
///
/// Rows with a missing/zero/unparseable surface coordinate are skipped. When
/// `lake_erie_only` is set, a row is kept only if its township contains
/// "lake erie" OR its coordinate is inside `LAKE_ERIE_BBOX`.
///
/// Returns an empty vec (with a logged warning) when `csv_path` does not exist.
pub fn load_ogsr_wells(csv_path: &Path, lake_erie_only: bool) -> Vec<Wellhead> {
    let mut wells: Vec<Wellhead> = Vec::new();

    if !csv_path.exists() {
        log::warn!("OGSr wells CSV not found: {}", csv_path.display());
        return wells;
    }

    let raw = match std::fs::read(csv_path) {
        Ok(b) => b,
        Err(e) => {
            log::warn!("OGSr wells CSV unreadable ({}): {e}", csv_path.display());
            return wells;
        }
    };
    let decoded = decode_cp1252(&raw);

    let mut reader = csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(true)
        .from_reader(decoded.as_bytes());

    // Build a header → column-index map so we can read columns by name with the
    // FULL_NAME/WELL_NAME and WELL_TYPE/CLASS fallbacks the Python loader uses.
    let headers = match reader.headers() {
        Ok(h) => h.clone(),
        Err(e) => {
            log::warn!("OGSr wells CSV header parse failed ({}): {e}", csv_path.display());
            return wells;
        }
    };
    let col = |name: &str| -> Option<usize> { headers.iter().position(|h| h == name) };
    let get = |rec: &csv::StringRecord, idx: Option<usize>| -> String {
        idx.and_then(|i| rec.get(i)).unwrap_or("").to_string()
    };

    let lat_idx = col("SUR_LAT83");
    let lon_idx = col("SUR_LONG83");
    let well_id_idx = col("WELL_ID");
    let full_name_idx = col("FULL_NAME");
    let well_name_idx = col("WELL_NAME");
    let status_idx = col("CUR_STATUS");
    let well_type_idx = col("WELL_TYPE");
    let class_idx = col("CLASS");
    let township_idx = col("TOWNSHIP");
    let county_idx = col("COUNTY");
    let target_idx = col("TARGET");

    let mut offshore = 0usize;

    for result in reader.records() {
        let rec = match result {
            Ok(r) => r,
            Err(_) => continue,
        };

        // Parse coordinates; treat empty/zero/non-numeric as "skip" (Python
        // `float(... or 0)` then the `lat == 0 or lon == 0` guard).
        let lat: f64 = match get(&rec, lat_idx).trim().parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let lon: f64 = match get(&rec, lon_idx).trim().parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        if lat == 0.0 || lon == 0.0 {
            continue;
        }

        let township = get(&rec, township_idx).trim().to_string();
        let is_lake = township.to_lowercase().contains("lake erie");

        if lake_erie_only {
            let in_bbox = LAKE_ERIE_BBOX.lat_min <= lat
                && lat <= LAKE_ERIE_BBOX.lat_max
                && LAKE_ERIE_BBOX.lon_min <= lon
                && lon <= LAKE_ERIE_BBOX.lon_max;
            if !(is_lake || in_bbox) {
                continue;
            }
        }

        // FULL_NAME with WELL_NAME fallback (Python `or`).
        let mut name = get(&rec, full_name_idx);
        if name.is_empty() {
            name = get(&rec, well_name_idx);
        }
        // WELL_TYPE with CLASS fallback (Python `or`).
        let mut well_type = get(&rec, well_type_idx);
        if well_type.is_empty() {
            well_type = get(&rec, class_idx);
        }

        if is_lake {
            offshore += 1;
        }
        wells.push(Wellhead {
            well_id: get(&rec, well_id_idx),
            name,
            lat,
            lon,
            status: get(&rec, status_idx),
            well_type,
            township,
            county: get(&rec, county_idx),
            target: get(&rec, target_idx),
            is_lake_erie: is_lake,
        });
    }

    log::info!(
        "Loaded {} wells from OGSr ({} offshore Lake Erie)",
        wells.len(),
        offshore
    );
    wells
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Write a temporary CSV file and return its path.
    fn write_temp_csv(name: &str, body: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!("ogsr_test_{}_{}.csv", std::process::id(), name));
        let mut f = std::fs::File::create(&path).expect("create temp csv");
        f.write_all(body.as_bytes()).expect("write temp csv");
        path
    }

    /// A synthetic OGSr CSV parses, and the Lake Erie filter keeps only the
    /// in-region rows (offshore township OR inside LAKE_ERIE_BBOX).
    #[test]
    fn test_synthetic_csv_parses_and_filters() {
        // Row 1: inside bbox (kept). Row 2: offshore "Lake Erie" township but
        // outside the bbox lon — kept via township. Row 3: far away (Toronto,
        // dropped). Row 4: zero coordinate (dropped). Row 5: blank coord (dropped).
        let body = "WELL_ID,FULL_NAME,SUR_LAT83,SUR_LONG83,CUR_STATUS,WELL_TYPE,TOWNSHIP,COUNTY,TARGET\n\
            W1,Coastal Well,42.10,-80.50,ACTIVE,GAS,Bayham,Elgin,Clinton\n\
            W2,Offshore Well,42.95,-78.50,ACTIVE,GAS,Lake Erie,Norfolk,Clinton\n\
            W3,Toronto Well,43.65,-79.38,ACTIVE,GAS,Toronto,York,Other\n\
            W4,Zero Well,0,0,ABANDONED,GAS,Bayham,Elgin,Clinton\n\
            W5,Blank Well,,,ABANDONED,GAS,Bayham,Elgin,Clinton\n";
        let path = write_temp_csv("filter", body);

        let wells = load_ogsr_wells(&path, true);
        let _ = std::fs::remove_file(&path);

        assert_eq!(wells.len(), 2, "only the two in-region wells should remain");
        assert!(wells.iter().any(|w| w.well_id == "W1" && !w.is_lake_erie));
        let offshore = wells
            .iter()
            .find(|w| w.well_id == "W2")
            .expect("offshore well kept via township");
        assert!(offshore.is_lake_erie, "Lake Erie township flags is_lake_erie");
        assert_eq!(offshore.name, "Offshore Well");
        assert_eq!(offshore.well_type, "GAS");
    }

    /// With `lake_erie_only = false`, every row with a valid coordinate is kept.
    #[test]
    fn test_no_filter_keeps_all_valid_rows() {
        let body = "WELL_ID,WELL_NAME,SUR_LAT83,SUR_LONG83,CLASS,TOWNSHIP\n\
            A,Alpha,42.10,-80.50,GAS,Bayham\n\
            B,Beta,43.65,-79.38,OIL,Toronto\n";
        let path = write_temp_csv("nofilter", body);

        let wells = load_ogsr_wells(&path, false);
        let _ = std::fs::remove_file(&path);

        assert_eq!(wells.len(), 2, "no filter keeps both valid rows");
        // WELL_NAME fallback when FULL_NAME column is absent.
        assert!(wells.iter().any(|w| w.name == "Alpha"));
        // CLASS fallback when WELL_TYPE column is absent.
        assert!(wells.iter().any(|w| w.well_type == "OIL"));
    }

    /// A missing file returns an empty vec (and logs a warning) rather than panicking.
    #[test]
    fn test_missing_file_returns_empty() {
        let path = std::path::Path::new("/nonexistent/ogsr_does_not_exist.csv");
        let wells = load_ogsr_wells(path, true);
        assert!(wells.is_empty(), "absent CSV yields empty vec");
    }

    /// cp1252-encoded bytes (e.g. 0x92 right single quote) decode without panic.
    #[test]
    fn test_cp1252_decoding() {
        // 0x92 is a cp1252 right single quote (U+2019); invalid UTF-8 on its own.
        let mut body: Vec<u8> =
            b"WELL_ID,FULL_NAME,SUR_LAT83,SUR_LONG83,TOWNSHIP\nW1,OBrien".to_vec();
        body.push(0x92); // smart apostrophe
        body.extend_from_slice(b"s Well,42.10,-80.50,Bayham\n");

        let mut path = std::env::temp_dir();
        path.push(format!("ogsr_cp1252_{}.csv", std::process::id()));
        std::fs::write(&path, &body).expect("write cp1252 csv");

        let wells = load_ogsr_wells(&path, true);
        let _ = std::fs::remove_file(&path);

        assert_eq!(wells.len(), 1);
        assert!(wells[0].name.contains('\u{2019}'), "cp1252 0x92 → U+2019");
    }
}
