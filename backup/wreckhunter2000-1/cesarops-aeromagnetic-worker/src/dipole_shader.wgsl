// WGSL Compute Shader: Magnetic Dipole Detection

struct Params {
    width: u32,
    height: u32,
    inner_radius: u32,
    outer_radius: u32,
    pixel_size_m: f32,   // meters per pixel
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> in_grid: array<f32>;
// Output is a struct for every pixel containing dipole features
struct OutPixel {
    bg_mean: f32,
    peak_pos: f32,
    peak_neg: f32,
    dipole_separation_m: f32,
    score: f32,          // final anomaly score
}
@group(0) @binding(2) var<storage, read_write> out_grid: array<OutPixel>;

const NAN_VAL: f32 = -9999.0;

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let x = global_id.x;
    let y = global_id.y;

    if x >= params.width || y >= params.height {
        return;
    }

    let center_idx = y * params.width + x;
    let center_val = in_grid[center_idx];

    // If candidate itself is NaN, skip
    if center_val <= NAN_VAL + 1.0 {
        out_grid[center_idx].score = 0.0;
        return;
    }

    var bg_sum: f32 = 0.0;
    var bg_count: f32 = 0.0;
    
    var inner_max: f32 = -999999.0;
    var inner_min: f32 = 999999.0;
    
    var max_x: f32 = f32(x);
    var max_y: f32 = f32(y);
    var min_x: f32 = f32(x);
    var min_y: f32 = f32(y);

    let start_y = max(0u, y - params.outer_radius);
    let end_y = min(params.height - 1u, y + params.outer_radius);
    let start_x = max(0u, x - params.outer_radius);
    let end_x = min(params.width - 1u, x + params.outer_radius);

    for (var cy = start_y; cy <= end_y; cy++) {
        for (var cx = start_x; cx <= end_x; cx++) {
            let dx = f32(cx) - f32(x);
            let dy = f32(cy) - f32(y);
            let dist_sq = dx * dx + dy * dy;
            let dist = sqrt(dist_sq);

            let idx = cy * params.width + cx;
            let val = in_grid[idx];

            if val > NAN_VAL + 1.0 {
                // Outer annulus
                if dist > f32(params.inner_radius) && dist <= f32(params.outer_radius) {
                    bg_sum += val;
                    bg_count += 1.0;
                }
                
                // Inner zone
                if dist <= f32(params.inner_radius) {
                    if val > inner_max {
                        inner_max = val;
                        max_x = f32(cx);
                        max_y = f32(cy);
                    }
                    if val < inner_min {
                        inner_min = val;
                        min_x = f32(cx);
                        min_y = f32(cy);
                    }
                }
            }
        }
    }

    var score = 0.0;
    var dipole_sep = 0.0;
    var bg_mean = 0.0;
    var peak_pos = 0.0;
    var peak_neg = 0.0;

    if bg_count > 0.0 {
        bg_mean = bg_sum / bg_count;
        peak_pos = inner_max - bg_mean;
        peak_neg = inner_min - bg_mean;
        
        // Signal Magnification: We sharpen the dipole flip by focusing on the 
        // distance between the absolute extrema (the "spine" of the hull signature).
        // Magnifying the peak difference helps reject background sediment noise.
        let peak_abs = max(peak_pos, abs(peak_neg));
        let dipole_mag = peak_pos - peak_neg; // Vertical separation of lobes

        if dipole_mag > 0.1 {
            let dx = max_x - min_x;
            let dy = max_y - min_y;
            dipole_sep = sqrt(dx * dx + dy * dy) * params.pixel_size_m;

            // Simple scoring function: ratio of lobe strengths
            // We favor targets where both lobes are roughly symmetric (ships/hulls).
            let lobe_ratio = min(peak_pos, abs(peak_neg)) / max(peak_pos, abs(peak_neg));
            score = dipole_mag * lobe_ratio;
        }
    }

    out_grid[center_idx].bg_mean = bg_mean;
    out_grid[center_idx].peak_pos = peak_pos;
    out_grid[center_idx].peak_neg = peak_neg;
    out_grid[center_idx].dipole_separation_m = dipole_sep;
    out_grid[center_idx].score = score;
}
