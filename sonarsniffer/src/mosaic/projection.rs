use crate::garmin_rsd_parser::ParseResult;
use crate::mosaic::grid::MosaicGrid;
use rayon::prelude::*;
use std::sync::Arc;

pub fn latlon_to_meters(lat: f64, lon: f64) -> (f64, f64) {
    let x = lon * 20037508.34 / 180.0;
    let mut y = (90.0 + lat) * std::f64::consts::PI / 360.0;
    y = y.tan().ln() / std::f64::consts::PI * 20037508.34;
    (x, y)
}

pub fn meters_to_latlon(x: f64, y: f64) -> (f64, f64) {
    let lon = x * 180.0 / 20037508.34;
    let y_norm = y * std::f64::consts::PI / 20037508.34;
    let lat = (f64::atan(f64::exp(y_norm)) * 2.0 - std::f64::consts::PI / 2.0) * 180.0 / std::f64::consts::PI;
    (lat, lon)
}

pub fn project_pings_to_grid(parse_res: &ParseResult, resolution_m: f32, _colormap: &str, remove_water_column: bool) -> Arc<MosaicGrid> {
    let mut min_x = f64::MAX;
    let mut max_x = f64::MIN;
    let mut min_y = f64::MAX;
    let mut max_y = f64::MIN;
    
    let max_samples = parse_res.pings.iter().map(|p| p.samples.len()).max().unwrap_or(0);
    let max_slant_range_m = max_samples as f64 * 0.04; 
    let buffer_m = max_slant_range_m * 1.5 + 50.0; 

    for p in &parse_res.pings {
        if p.latitude == 0.0 || p.longitude == 0.0 { continue; }
        let (x, y) = latlon_to_meters(p.latitude, p.longitude);
        if x < min_x { min_x = x; }
        if x > max_x { max_x = x; }
        if y < min_y { min_y = y; }
        if y > max_y { max_y = y; }
    }
    
    if min_x == f64::MAX { min_x = 0.0; max_x = 100.0; min_y = 0.0; max_y = 100.0; }

    min_x -= buffer_m;
    max_x += buffer_m;
    min_y -= buffer_m;
    max_y += buffer_m;

    let res = if resolution_m > 0.0 { resolution_m as f64 } else { 0.25 };
    eprintln!("[grid] Building map grid: bounds ({:.1},{:.1}) to ({:.1},{:.1}) at res {}m", min_x, min_y, max_x, max_y, res);
    let grid = Arc::new(MosaicGrid::new(min_x, min_y, max_x, max_y, res));
    eprintln!("[grid] Initialized array size: {}x{} pixels", grid.width, grid.height);
    
    parse_res.pings.par_iter().for_each(|ping| {
        if ping.latitude == 0.0 || ping.longitude == 0.0 { return; }

        let (cx, cy) = latlon_to_meters(ping.latitude, ping.longitude);
        let heading = ping.heading_deg.unwrap_or(0.0) as f64;
        
        let chan_info = parse_res.channels.iter().find(|c| c.id == ping.channel);
        let mapped_type = chan_info.and_then(|c| c.mapped_type.as_deref()).unwrap_or("unknown");

        let angle_offset_deg = match mapped_type {
            "port_sidescan" => -90.0,
            "starboard_sidescan" => 90.0,
            "chirp_downscan" => 0.0,
            "depth_temp" => return,
            _ => return, 
        };

        let true_angle_deg = heading + angle_offset_deg;
        let math_rad = (90.0 - true_angle_deg).to_radians();
        let cos_a = math_rad.cos();
        let sin_a = math_rad.sin();

        let n = ping.samples.len();
        if n == 0 { return; }
        
        let depth_m = ping.depth_m as f64;
        // Use the same 0.01 m/sample constant as the outputs.rs swath calculations
        // (SONAR_M_PER_SAMPLE_F64). Using 0.035 was 3.5× too large, projecting data
        // far beyond the actual sonar range and producing an oversized sparse grid.
        let sample_resolution_m = 0.01;

        // Maximum horizontal reach for this ping (used to build a continuous scanline).
        let max_slant_m = (n.saturating_sub(1) as f64) * sample_resolution_m;
        let max_ground_m = if max_slant_m > depth_m {
            (max_slant_m * max_slant_m - depth_m * depth_m).sqrt()
        } else {
            0.0
        };
        if max_ground_m <= 0.0 { return; }

        let ground_scale = if mapped_type == "chirp_downscan" { 0.1 } else { 1.0 };
        let max_ground_proj = max_ground_m * ground_scale;

        // Step along the perpendicular scanline at roughly grid resolution to avoid holes.
        let dx = cos_a * max_ground_proj;
        let dy = sin_a * max_ground_proj;
        let steps = ((dx.abs().max(dy.abs()) / grid.resolution).ceil() as usize).max(1);
        let sigma = (max_ground_proj * 0.35).max(0.25); // beam feather width

        for step in 0..=steps {
            let frac = step as f64 / steps as f64;
            let ground_m = frac * max_ground_m; // true horizontal distance on seafloor
            let slant_m = (ground_m * ground_m + depth_m * depth_m).sqrt();

            if slant_m <= depth_m && remove_water_column {
                continue; // skip water column when requested
            }

            // Sample intensity along the slant path with simple linear interpolation.
            let sample_pos = slant_m / sample_resolution_m;
            let base = sample_pos.floor() as usize;
            let intensity: f32 = if base + 1 < n {
                let t = (sample_pos - base as f64) as f32;
                let a = ping.samples[base] as f32;
                let b = ping.samples[base + 1] as f32;
                a + (b - a) * t
            } else {
                ping.samples[n - 1] as f32
            };

            // Gaussian feathering toward beam edges (weight highest near nadir/center).
            let weight = (-((ground_m * ground_scale).powi(2) / (2.0 * sigma * sigma))).exp() as f32;

            let proj_ground = ground_m * ground_scale;
            let px = cx + proj_ground * cos_a;
            let py = cy + proj_ground * sin_a;

            grid.add_weighted_sample(px, py, intensity, weight);
        }
    });

    eprintln!("[grid] Finished projecting {} pings.", parse_res.pings.len());
    grid
}
