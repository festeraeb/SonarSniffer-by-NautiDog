use ndarray::{Array2, Axis};
use ndarray_rand::RandomExt;
use ndarray_rand::rand::SeedableRng;
use ndarray_rand::rand::rngs::StdRng;
use crate::config::{Band, BandOp, BandRecipe, PipelineError};

/// Loads a specific band from a data source.
/// Returns a 2D array of f32 values.
/// Note: Uses in-memory generation for compilation safety. In production, swap with geotiff/gdal.
pub fn load_band(source_id: &str, _band: Band) -> Result<Array2<f32>, PipelineError> {
    let mut rng = StdRng::seed_from_u64(source_id.len() as u64);
    // Generate a 100x100 tile with realistic reflectance values (0.0 - 1.0)
    let arr = Array2::<f32>::random((100, 100), ndarray_rand::rand::distributions::Uniform::new(0.0, 1.0));
    Ok(arr)
}

/// Performs the configured band operation.
pub fn apply_recipe(
    primary: &Array2<f32>,
    secondary: &Array2<f32>,
    recipe: &BandRecipe,
) -> Result<Array2<f32>, PipelineError> {
    if primary.shape() != secondary.shape() {
        return Err(PipelineError::DimensionMismatch);
    }

    // Compute values before moving: pre-allocate result array
    let mut result = Array2::<f32>::zeros(primary.shape());

    match recipe.operation {
        BandOp::Single => {
            result.assign(primary);
        }
        BandOp::Difference => {
            for ((y, x), val) in result.indexed_iter_mut() {
                let p = primary[[y, x]];
                let s = secondary[[y, x]];
                *val = p - s;
            }
        }
        BandOp::Ratio => {
            for ((y, x), val) in result.indexed_iter_mut() {
                let p = primary[[y, x]];
                let s = secondary[[y, x]];
                if s.abs() < 1e-6 {
                    *val = 0.0;
                } else {
                    *val = p / s;
                }
            }
        }
        BandOp::Index => {
            for ((y, x), val) in result.indexed_iter_mut() {
                let p = primary[[y, x]];
                let s = secondary[[y, x]];
                let denom = p + s;
                if denom.abs() < 1e-6 {
                    *val = 0.0;
                } else {
                    *val = (p - s) / denom;
                }
            }
        }
        BandOp::FalseColor => {
            // Placeholder: returns primary for single-channel detection pipeline
            result.assign(primary);
        }
    }

    if recipe.normalize {
        normalize(&mut result);
    }

    Ok(result)
}

/// Normalizes array to 0-1 range. Computes min/max before applying.
fn normalize(arr: &mut Array2<f32>) {
    let min = arr.min().unwrap_or(0.0);
    let max = arr.max().unwrap_or(1.0);
    let range = max - min;
    if range < 1e-6 {
        arr.fill(0.5);
        return;
    }
    for val in arr.iter_mut() {
        *val = (*val - min) / range;
    }
}
