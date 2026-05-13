use ndarray::prelude::*;
use ndarray_rand::RandomExt;
use ndarray_rand::rand::distributions::Bernoulli;
use ndarray_rand::rand::thread_rng;

fn main() {
    // Fix 1: Use a fixed-size array or explicit Dim for shape to resolve ShapeBuilder mismatch
    // Instead of &[usize], use &[usize; 2] or ArrayBase::from_shape_vec with explicit dims
    let shape = [2, 3];
    let mut arr = Array::<f32, _>::zeros(shape.into());
    
    // Fix 2: If you need to generate random data, use a distribution compatible with the type.
    // For f32, Standard is fine. For bool, use Bernoulli.
    // Here we assume we might have had a bool array generation error.
    // Let's create a bool array using Bernoulli(0.5)
    let mut bool_arr = Array::<bool, _>::from_shape_fn([2, 3], |_| {
        thread_rng().sample(Bernoulli::new(0.5).unwrap())
    });

    // Fix 3: min and max methods are not directly available on &mut ArrayBase.
    // Use min_axis and max_axis instead.
    // Note: min_axis returns an Option<Axis> and the value is found via indexing or min_axis().into()
    // Actually, min_axis() returns Option<(usize, f32)> for f32 arrays.
    
    // Example for f32 array:
    if let Some((axis, val)) = arr.min_axis() {
        println!("Min value: {}, Axis: {:?}", val, axis);
    }
    
    if let Some((axis, val)) = arr.max_axis() {
        println!("Max value: {}, Axis: {:?}", val, axis);
    }

    // Example for bool array:
    // Booleans don't have min/max in the same numerical sense, but you can count true/false
    let true_count = bool_arr.iter().filter(|&&b| b).count();
    println!("True count: {}", true_count);
}
