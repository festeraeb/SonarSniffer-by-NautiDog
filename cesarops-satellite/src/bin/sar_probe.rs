//! sar_probe — quick check that the pure-Rust geotiff reader opens an RTC VV
//! tile and the SAR window extraction yields sane backscatter + coordinates.
//! Usage: sar-probe <vv.tif> <lat_min> <lon_min> <lat_max> <lon_max>
use cesarops_satellite::geotiff;
use cesarops_satellite::types::BBox;

fn main() -> anyhow::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    if a.len() < 6 {
        eprintln!("usage: sar-probe <vv.tif> <lat_min> <lon_min> <lat_max> <lon_max>");
        std::process::exit(2);
    }
    let path = std::path::Path::new(&a[1]);
    let bbox = BBox {
        lat_min: a[2].parse()?,
        lon_min: a[3].parse()?,
        lat_max: a[4].parse()?,
        lon_max: a[5].parse()?,
    };
    let geo = geotiff::read_georef(path)?;
    println!("georef: epsg={} {}x{} scale=({},{}) origin=({:.1},{:.1}) utm_zone={:?}",
        geo.epsg, geo.width, geo.height, geo.scale_x, geo.scale_y,
        geo.origin_x, geo.origin_y, geo.utm_zone());
    let (arr, _g, wx, wy) = geotiff::read_window_raw(path, &bbox)?;
    let (h, w) = arr.dim();
    let finite: Vec<f32> = arr.iter().copied().filter(|v| v.is_finite() && *v > 0.0).collect();
    println!("window: origin=({wx},{wy}) size={w}x{h}  finite_px={}", finite.len());
    if !finite.is_empty() {
        let mn = finite.iter().cloned().fold(f32::INFINITY, f32::min);
        let mx = finite.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let mean = finite.iter().sum::<f32>() / finite.len() as f32;
        println!("backscatter: min={mn:.4} max={mx:.4} mean={mean:.4}");
    }
    // corner coords
    if let Some((lat, lon)) = geotiff::pixel_to_wgs84(&geo, wy as f64, wx as f64) {
        println!("window NW corner -> lat {lat:.5} lon {lon:.5}");
    }
    Ok(())
}
