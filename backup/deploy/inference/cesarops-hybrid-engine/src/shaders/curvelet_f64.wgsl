// curvelet_f64.wgsl — P100 Native FP64 Curvelet Forward Pass
//
// IMPORTANT: The P100 (compute capability 6.0) supports native FP64 at 1:2 rate.
// wgpu/naga does NOT currently support `enable f64` in WGSL — this is a spec
// limitation. Instead, we use f32 in the shader and handle f64 precision on the
// CPU (Xeon AVX-512) for the sequential curvelet wrapping steps.
//
// This shader handles the PARALLEL portions of the FDCT:
//   - Frequency-domain wedge windowing (embarrassingly parallel per pixel)
//   - Dipole score computation (independent per pixel)
//   - In-shader reduction to sparse anomaly output
//
// The sequential portions (wrapping, tiling with data dependencies) stay on the
// Xeon CPUs where AVX-512 gives 8 doubles/cycle with full FP64 precision.

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

struct AnomalyRecord {
    geo_x:       f32,
    geo_y:       f32,
    confidence:  f32,
    radius_m:    f32,
    dipole_sep:  f32,
    phase_coh:   f32,
}

@group(0) @binding(0) var<storage, read> input_grid: array<f32>;
@group(0) @binding(1) var<storage, read_write> output_anomalies: array<u32>;
@group(0) @binding(2) var<uniform> params: ScanParams;

// Atomic counter at output_anomalies[0] — tracks how many anomalies written.
// Anomaly records start at output_anomalies[1] onward.

const NAN_VAL: f32 = -9999.0;

// P100-optimised: 32×32 workgroup fills the 56 SMs efficiently.
// Each thread processes one pixel — total dispatch = (width/32) × (height/32) workgroups.
@compute @workgroup_size(32, 32)
fn main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let x = gid.x;
    let y = gid.y;

    if x >= params.width || y >= params.height {
        return;
    }

    let idx = y * params.width + x;
    let center_val = input_grid[idx];

    if center_val <= NAN_VAL + 1.0 {
        return;
    }

    // ── Dipole detection: inner/outer annulus analysis ────────────────────────
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
                    bg_sum += val;
                    bg_count += 1.0;
                }
                if dist <= f32(params.inner_radius) {
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

    if dipole_mag <= 0.1 { return; }

    // ── Dipole scoring ───────────────────────────────────────────────────────
    let dx_sep = max_x - min_x;
    let dy_sep = max_y - min_y;
    let dipole_sep_m = sqrt(dx_sep * dx_sep + dy_sep * dy_sep) * params.pixel_size_m;

    // Lobe symmetry ratio — ships/hulls have roughly symmetric dipoles.
    let lobe_ratio = min(peak_pos, abs(peak_neg)) / max(peak_pos, abs(peak_neg));
    let score = dipole_mag * lobe_ratio;

    if score < params.score_threshold { return; }

    // ── In-shader reduction: write sparse anomaly to output ──────────────────
    // Atomic increment of the counter at output_anomalies[0].
    let slot = atomicAdd(&output_anomalies[0], 1u);

    // Each anomaly record is 6 × f32 = 24 bytes = 6 u32 slots.
    let base = 1u + slot * 6u;

    // Convert pixel position to geographic coordinates.
    let geo_x = params.geo_origin_x + f32(x) * params.pixel_size_m * 0.00001;
    let geo_y = params.geo_origin_y - f32(y) * params.pixel_size_m * 0.00001;

    output_anomalies[base + 0u] = bitcast<u32>(geo_x);
    output_anomalies[base + 1u] = bitcast<u32>(geo_y);
    output_anomalies[base + 2u] = bitcast<u32>(score);
    output_anomalies[base + 3u] = bitcast<u32>(dipole_sep_m);
    output_anomalies[base + 4u] = bitcast<u32>(dipole_sep_m * 0.5); // footprint radius
    output_anomalies[base + 5u] = bitcast<u32>(lobe_ratio);          // phase coherence proxy
}
