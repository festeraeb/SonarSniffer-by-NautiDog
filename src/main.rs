use ndarray::prelude::*;

fn main() {
    // Create a fixed-size array for demonstration
    let shape = [2, 3];
    let mut arr = Array::<f32, _>::zeros(shape);
    
    // Manually set some values since RandomExt may cause additional import warnings
    arr[[0, 0]] = 5.0;
    arr[[0, 1]] = 3.0;
    arr[[0, 2]] = 8.0;
    arr[[1, 0]] = 1.0;
    arr[[1, 1]] = 7.0;
    arr[[1, 2]] = 4.0;

    // Find min using iterator (ndarray 0.15 doesn't have min_axis directly)
    if let Some((axis_idx, val)) = arr.iter().enumerate().min_by(|a, b| a.1.partial_cmp(b.1).unwrap()) {
        println!("Min value: {} at index {}", val, axis_idx);
    }
    
    // Find max using iterator
    if let Some((axis_idx, val)) = arr.iter().enumerate().max_by(|a, b| a.1.partial_cmp(b.1).unwrap()) {
        println!("Max value: {} at index {}", val, axis_idx);
    }

    // Example for bool array counting true values
    let bool_arr = Array::<bool, _>::from_shape_fn([2, 3], |_| false);
    let true_count = bool_arr.iter().filter(|&&b| b).count();
    println!("True count: {}", true_count);
}
