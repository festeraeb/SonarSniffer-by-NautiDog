//! Sensor-lane math blueprints (nautivecs source of truth). All spatial/radiometric outputs use f64.

/// Secchi disk depth (Z_sd) from remote-sensing reflectance proxy (Lee-style K_d inversion).
pub fn calculate_secchi_depth(r_rs_443: f64, r_rs_555: f64) -> f64 {
    let a_w = 0.007;
    let b_bw = 0.00093;
    let ratio = r_rs_443 / r_rs_555;
    let a_phi = 0.06 * ratio.powf(-1.2);
    let total_a = a_w + a_phi;
    let total_bb = b_bw + (0.01 * (r_rs_555 / 0.01).powf(-0.5));
    let k_d = (1.0 + 4.16 * total_a).powf(0.5) * total_a
        + (1.0 - 0.52 * (total_a / (total_a + total_bb))) * total_bb;
    let alpha = 1.48;
    alpha / k_d
}

/// Bragg wavelength for Sentinel-1 C-band (λ_radar = 5.6 cm).
pub fn calculate_bragg_wavelength(incidence_angle_rad: f64) -> f64 {
    let lambda_radar = 0.056;
    lambda_radar / (2.0 * incidence_angle_rad.sin())
}

/// Marangoni slick damping proxy: dB drop vs ambient σ⁰.
pub fn calculate_slick_damping_ratio(sigma_ambient: f64, sigma_observed: f64) -> f64 {
    10.0 * (sigma_ambient / sigma_observed).log10()
}

/// Landsat 8/9 Band 10 brightness temperature (K) from USGS DN.
pub fn landsat_b10_brightness_temp(dn: f64) -> f64 {
    let ml = 0.0003342;
    let al = 0.1;
    let l_lambda = (ml * dn) + al;
    let k1 = 774.89;
    let k2 = 1321.08;
    k2 / ((k1 / l_lambda) + 1.0).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secchi_positive_for_typical_rrs() {
        let z = calculate_secchi_depth(0.01, 0.008);
        assert!(z.is_finite() && z > 0.0);
    }

    #[test]
    fn bragg_wavelength_order_of_meters() {
        let lam = calculate_bragg_wavelength(0.5);
        assert!(lam > 0.05 && lam < 0.2);
    }

    #[test]
    fn planck_temp_reasonable_kelvin() {
        let t = landsat_b10_brightness_temp(30000.0);
        assert!(t > 250.0 && t < 330.0);
    }
}
