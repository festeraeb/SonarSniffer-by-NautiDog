mod coeffs;
mod config;
mod error;
mod fft;
mod forward;
mod inverse;
mod utils;
mod windows;
mod wrapping;
pub use coeffs::CurveletCoeffs;
pub use config::CurveletConfig;
pub use error::CurveletError;
use ndarray::Array2;
pub fn curvelet_forward(
    image: &Array2<f32>,
    num_scales: usize,
) -> Result<CurveletCoeffs, CurveletError> {
    let config = CurveletConfig::new(num_scales)?;
    curvelet_forward_config(image, &config)
}
pub fn curvelet_forward_config(
    image: &Array2<f32>,
    config: &CurveletConfig,
) -> Result<CurveletCoeffs, CurveletError> {
    forward::forward_transform(image, config)
}
pub fn curvelet_inverse(coeffs: &CurveletCoeffs) -> Result<Array2<f32>, CurveletError> {
    inverse::inverse_transform(coeffs)
}
#[cfg(test)]
mod tests;