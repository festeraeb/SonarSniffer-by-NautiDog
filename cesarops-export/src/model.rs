use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone)]
pub struct ExportCandidate {
    pub id: String,
    pub lat: f64,
    pub lon: f64,
    pub source: String,
    pub confidence: f64,
    pub long_ft: Option<f64>,
    pub short_ft: Option<f64>,
    pub depth_ft: Option<f64>,
    pub relief_ft: Option<f64>,
    pub signature: Option<String>,
    pub notes: String,
    pub thumb_png: Option<String>,
    pub metrics_json: String,
}

#[derive(Debug, Deserialize)]
pub struct BagMissionReport {
    pub detections: Vec<BagDetection>,
}

#[derive(Debug, Deserialize)]
pub struct BagDetection {
    pub id: String,
    pub signature_type: String,
    pub latitude: f64,
    pub longitude: f64,
    pub size_sq_feet: f64,
    pub long_side_ft: f64,
    pub short_side_ft: f64,
    pub depth_meters: f64,
    pub height_above_floor_m: f64,
    pub confidence: f64,
    pub object_type: Value,
    pub metadata: Value,
}

impl BagDetection {
    pub fn into_export(self, thumb_png: Option<String>) -> ExportCandidate {
        let source = match self.signature_type.as_str() {
            "physical_wreck" => "bag_physical",
            "masked_redaction_flat" => "bag_masked",
            other => other,
        }
        .to_string();
        let notes = format!(
            "object_type={} size_sq_ft={:.1}",
            self.object_type, self.size_sq_feet
        );
        let metrics_json =
            serde_json::to_string(&serde_json::json!({
                "id": self.id,
                "signature_type": self.signature_type,
                "latitude": self.latitude,
                "longitude": self.longitude,
                "size_sq_feet": self.size_sq_feet,
                "long_side_ft": self.long_side_ft,
                "short_side_ft": self.short_side_ft,
                "depth_meters": self.depth_meters,
                "height_above_floor_m": self.height_above_floor_m,
                "confidence": self.confidence,
                "object_type": self.object_type,
                "metadata": self.metadata,
            }))
            .unwrap_or_else(|_| "{}".to_string());

        ExportCandidate {
            id: format!("bag:{}", self.id),
            lat: self.latitude,
            lon: self.longitude,
            source,
            confidence: self.confidence,
            long_ft: Some(self.long_side_ft),
            short_ft: Some(self.short_side_ft),
            depth_ft: Some(self.depth_meters * 3.28084),
            relief_ft: Some(self.height_above_floor_m * 3.28084),
            signature: Some(self.signature_type),
            notes,
            thumb_png,
            metrics_json,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct KnownWreckEntry {
    pub name: String,
    #[serde(default)]
    pub lat_min: Option<f64>,
    #[serde(default)]
    pub lat_max: Option<f64>,
    #[serde(default)]
    pub lon_min: Option<f64>,
    #[serde(default)]
    pub lon_max: Option<f64>,
    #[serde(default)]
    pub depth_ft: Option<f64>,
    #[serde(default)]
    pub r#type: Option<String>,
    #[serde(default)]
    pub confidence: Option<String>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl KnownWreckEntry {
    pub fn has_coords(&self) -> bool {
        self.lat_min.is_some() && self.lat_max.is_some() && self.lon_min.is_some() && self.lon_max.is_some()
    }

    pub fn into_export(self, key: &str) -> Option<ExportCandidate> {
        let (lat_min, lat_max, lon_min, lon_max) = (
            self.lat_min?,
            self.lat_max?,
            self.lon_min?,
            self.lon_max?,
        );
        let lat = (lat_min + lat_max) / 2.0;
        let lon = (lon_min + lon_max) / 2.0;
        let metrics_json = serde_json::to_string(&serde_json::json!({
            "key": key,
            "name": self.name,
            "lat_min": lat_min,
            "lat_max": lat_max,
            "lon_min": lon_min,
            "lon_max": lon_max,
            "depth_ft": self.depth_ft,
            "type": self.r#type,
            "confidence": self.confidence,
            "source": self.source,
            "notes": self.notes,
        }))
        .unwrap_or_else(|_| "{}".to_string());
        Some(ExportCandidate {
            id: format!("gt:{key}"),
            lat,
            lon,
            source: "ground_truth".to_string(),
            confidence: 1.0,
            long_ft: None,
            short_ft: None,
            depth_ft: self.depth_ft,
            relief_ft: None,
            signature: self.r#type,
            notes: self
                .notes
                .unwrap_or_else(|| self.name.clone()),
            thumb_png: None,
            metrics_json,
        })
    }
}

pub fn parse_bag_report(path: &std::path::Path) -> Result<Vec<ExportCandidate>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let report: BagMissionReport = serde_json::from_str(&text)?;
    Ok(report
        .detections
        .into_iter()
        .map(|d| d.into_export(None))
        .collect())
}

pub fn parse_ground_truth(path: &std::path::Path) -> Result<Vec<ExportCandidate>, Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(path)?;
    let raw: std::collections::HashMap<String, KnownWreckEntry> = serde_json::from_str(&text)?;
    Ok(raw
        .into_iter()
        .filter_map(|(k, e)| e.into_export(&k))
        .collect())
}
