use crate::geotile::GeoTile;
use reqwest::blocking::Client;
use serde_json::json;
use std::error::Error;
use std::io::Cursor;

/// Post a GeoTile to the TPU server for inference.
/// The tile is PNG-encoded in-memory and sent as base64 in JSON with geo metadata.
pub fn post_tile_for_inference(server_url: &str, tile: &GeoTile) -> Result<serde_json::Value, Box<dyn Error>> {
    // Encode tile data as grayscale PNG in-memory
    let mut buf: Vec<u8> = Vec::new();
    {
        let mut imgbuf = image::GrayImage::new(tile.width as u32, tile.height as u32);
        for r in 0..tile.height {
            for c in 0..tile.width {
                let v = tile.data[r * tile.width + c];
                // Normalize / clamp to 0-255 for PNG preview
                let iv = if v.is_finite() {
                    let scaled = ((v - (-100.0)) / (100.0 - (-100.0))) * 255.0;
                    scaled.max(0.0).min(255.0) as u8
                } else { 0u8 };
                imgbuf.put_pixel(c as u32, r as u32, image::Luma([iv]));
            }
        }
        let mut cursor = Cursor::new(&mut buf);
        image::DynamicImage::ImageLuma8(imgbuf).write_to(&mut cursor, image::ImageOutputFormat::Png)?;
    }

    // Use standard base64 encoding
    let b64 = base64::encode(&buf);

    let meta = json!({
        "crs": tile.crs,
        "geotransform": tile.geotransform,
        "width": tile.width,
        "height": tile.height,
    });

    let payload = json!({
        "image_base64": b64,
        "meta": meta
    });

    let client = Client::new();
    let resp = client.post(server_url).json(&payload).send()?;
    let status = resp.status();
    let j: serde_json::Value = resp.json()?;
    if !status.is_success() {
        Err(format!("TPU server returned {}: {}", status, j).into())
    } else {
        Ok(j)
    }
}
