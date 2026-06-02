use reqwest::{Client, Url};
use stac::{Item, Links};
use serde_json::Value;
use std::error::Error;
use std::io::Cursor;
use tiff::decoder::{Decoder, DecodingResult};
use tiff::ColorType;

/// The Planetary Computer URL base
const MPC_STAC_URL: &str = "https://planetarycomputer.microsoft.com/api/stac/v1";

/// Download a specific asset from a STAC item as an array of f32
pub async fn download_asset_as_f32(client: &Client, stac_item_url: &str, asset_key: &str) -> Result<Vec<f32>, Box<dyn Error>> {
    // 1. Get the STAC item
    let item: Item = client.get(stac_item_url).send().await?.json().await?;
    
    // 2. Find the requested asset
    let asset = item.assets.get(asset_key)
        .ok_or_else(|| format!("Asset {} not found in STAC item", asset_key))?;
    
    let href = &asset.href;
    
    // 3. Download the actual image (Stream or full buffer for this prototype)
    let img_bytes = client.get(href).send().await?.bytes().await?;
    
    // 4. Decode the TIFF
    let cursor = Cursor::new(img_bytes);
    let mut decoder = Decoder::new(cursor)?;
    
    let (width, height) = decoder.dimensions()?;
    let color_type = decoder.colortype()?;
    
    // 5. Expand into flat f32 vec
    let result = decoder.read_image()?;
    
    let flat_data: Vec<f32> = match result {
        DecodingResult::U8(data) => data.into_iter().map(|v| v as f32 / 255.0).collect(),
        DecodingResult::U16(data) => data.into_iter().map(|v| v as f32 / 65535.0).collect(),
        DecodingResult::F32(data) => data,
        _ => return Err("Unsupported TIFF format".into())
    };
    
    Ok(flat_data)
}

/// Simulated helper to mock downloads since querying actual items can be brittle in dev
pub fn fetch_mock_tile() -> Vec<f32> {
    // A mock 100x100 tile
    vec![0.05; 100 * 100]
}
