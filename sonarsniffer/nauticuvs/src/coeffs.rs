use ndarray::Array2;
use num_complex::{Complex, ComplexFloat};
use crate::config::CurveletConfig;
use crate::error::CurveletError;
#[derive(Debug, Clone)]
pub struct CurveletCoeffs {
    pub coarse: Array2<Complex<f64>>,
    pub detail: Vec<Vec<Array2<Complex<f64>>>>,
    pub fine: Array2<Complex<f64>>,
    pub(crate) config: CurveletConfig,
}
impl CurveletCoeffs {
    pub fn num_coeffs(&self) -> usize {
        let coarse_n = self.coarse.len();
        let fine_n = self.fine.len();
        let detail_n: usize = self
            .detail
            .iter()
            .flat_map(|scale| scale.iter())
            .map(|sb| sb.len())
            .sum();
        coarse_n + detail_n + fine_n
    }
    pub(crate) fn validate(&self) -> Result<(), CurveletError> {
        let n_detail = self.config.num_detail_scales();
        if self.detail.len() != n_detail {
            return Err(CurveletError::InconsistentCoeffs(format!(
                "expected {} detail scales, got {}",
                n_detail,
                self.detail.len()
            )));
        }
        for (i, scale) in self.detail.iter().enumerate() {
            let expected_dirs = self.config.directions_at_detail_scale(i);
            if scale.len() != expected_dirs {
                return Err(CurveletError::InconsistentCoeffs(format!(
                    "detail scale {} expected {} directions, got {}",
                    i,
                    expected_dirs,
                    scale.len()
                )));
            }
        }
        Ok(())
    }
    pub fn hard_threshold(&mut self, threshold: f64) {
        for scale in &mut self.detail {
            for subband in scale {
                for c in subband.iter_mut() {
                    if c.abs() < threshold {
                        *c = Complex::ZERO;
                    }
                }
            }
        }
    }
    pub fn soft_threshold(&mut self, threshold: f64) {
        for scale in &mut self.detail {
            for subband in scale {
                for c in subband.iter_mut() {
                    let mag = c.abs();
                    if mag < threshold {
                        *c = Complex::ZERO;
                    } else {
                        *c *= (mag - threshold) / mag;
                    }
                }
            }
        }
    }
}