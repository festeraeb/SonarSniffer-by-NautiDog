use anyhow::Result;

/// Converts raw WKB (Well-Known Binary) bytes (e.g. directly from PostGIS ST_AsBinary)
/// into a GeoJSON String using the `geozero` trait structure.
pub fn wkb_to_geojson(wkb_bytes: &[u8]) -> Result<String> {
    use geozero::wkb::Wkb;
    use geozero::{GeozeroDatasource, ToJson};

    let wkb = Wkb(wkb_bytes.to_vec());
    let json_string = wkb.to_json()?;
    Ok(json_string)
}
