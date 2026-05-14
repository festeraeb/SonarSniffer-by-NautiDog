// nauticus_scanner.wgsl — Nauticus Sub-surface Signature Scanner
//
// P100-optimised compute shader for aeromagnetic/thermal/spectral anomaly detection.
// Implements the dipole signature detection algorithm with in-shader sparse reduction.
//
// Workgroup size: 32×32 — fills P100's 56 SMs efficiently.
// Input: magnetic/thermal/spectral grid (f32 per pixel)
// Output: sparse anomaly records (atomic append to output buffer)

struct ScanParams {
    width:           u32,
    height:          u32,
    inner_radius:    u32,
    outer_radius:    u32,
    pixel_size_m:    f32,
    score_threshold: f32,
    geo_origin_x:    f32,
    geo_origin_y:    f32,
}

@group(0) @binding(0) var<storage, read> input_grid: array<f32>;
@group(0) @binding(1) var<storage, read_write> output_anomalies: array<u32>;
@group(0) @binding(2) var<uniform> params: ScanParams;

// Sentinel value for no-data pixels (NaN equivalent in integer grids).
const NAN_VAL: f32 = -9999.0;

// P100-optimised: 32×32 workgroup fills the 56 SMs efficiently.
// Each thread processes one pixel — total dispatch = (width/32) × (height/32) workgroups.
@compute @workgroup_size(32, 32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;

    // Bounds check — threads outside the grid do nothing.
    if x >= params.width || y >= params.height {
        return;
    }

    let idx = y * params.width + x;
    let center_val = input_grid[idx];

    // Skip no-data pixels.
    if center_val <= NAN_VAL + 1.0 {
        return;
    }

    // ── Dipole detection: inner/outer annulus analysis ────────────────────────
    // Inner annulus: look for positive and negative lobes (dipole signature).
    // Outer annulus: compute background mean for anomaly contrast.
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
            let dist = sqrt(dx * dx + dy * dy);
            let val = input_grid[cy * params.width + cx];

            if val > NAN_VAL + 1.0 {
                if dist > f32(params.inner_radius) && dist <= f32(params.outer_radius) {
                    // Outer annulus — background statistics.
                    bg_sum += val;
                    bg_count += 1.0;
                }
                if dist <= f32(params.inner_radius) {
                    // Inner annulus — track positive and negative lobes.
                    if val > inner_max { inner_max = val; max_x = f32(cx); max_y = f32(cy); }
                    if val < inner_min { inner_min = val; min_x = f32(cx); min_y = f32(cy); }
                }
            }
        }
    }

    if bg_count < 1.0 { return; }

    let bg_mean = bg_sum / bg_count;
    let peak_pos = inner_max - bg_mean;
    let peak_neg = inner_min - bg_mean;
    let dipole_mag = peak_pos - peak_neg;

    // Minimum dipole magnitude threshold — reject noise.
    if dipole_mag <= 0.1 { return; }

    // ── Dipole scoring ───────────────────────────────────────────────────────
    let dx_sep = max_x - min_x;
    let dy_sep = max_y - min_y;
    let dipole_sep_m = sqrt(dx_sep * dx_sep + dy_sep * dy_sep) * params.pixel_size_m;

    // Lobe symmetry ratio — ships/hulls have roughly symmetric dipoles.
    // Geological features tend to be asymmetric.
    let lobe_ratio = min(peak_pos, abs(peak_neg)) / max(peak_pos, abs(peak_neg));
    let score = dipole_mag * lobe_ratio;

    if score < params.score_threshold { return; }

    // ── In-shader reduction: write sparse anomaly to output ──────────────────
    // Atomic increment of the counter at output_anomalies[0].
    let slot = atomicAdd(&output_anomalies[0], 1u);

    // Each anomaly record is 6 × f32 = 24 bytes = 6 u32 slots.
    let base = 1u + slot * 6u;

    // Convert pixel position to geographic coordinates.
    // Approximate: pixel offset × pixel_size_m × degrees_per_metre.
    let geo_x = params.geo_origin_x + f32(x) * params.pixel_size_m * 0.00001;
    let geo_y = params.geo_origin_y - f32(y) * params.pixel_size_m * 0.00001;

    output_anomalies[base + 0u] = bitcast<u32>(geo_x);
    output_anomalies[base + 1u] = bitcast<u32>(geo_y);
    output_anomalies[base + 2u] = bitcast<u32>(score);
    output_anomalies[base + 3u] = bitcast<u32>(dipole_sep_m);
    output_anomalies[base + 4u] = bitcast<u32>(dipole_sep_m * 0.5); // footprint radius estimate
    output_anomalies[base + 5u] = bitcast<u32>(lobe_ratio);          // phase coherence proxy
}
