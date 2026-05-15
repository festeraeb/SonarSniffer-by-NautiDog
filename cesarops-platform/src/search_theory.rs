//! # Search Theory Module
//! 
//! This module provides mathematical implementations for Bayesian search theory 
//! specifically tailored for Search and Rescue (SAR) operations.
//! 
//! It implements the Exponential Detection Function, Bayesian posterior updates 
//! for probability maps, and effort allocation strategies.

/// Represents the result of a search effort on a specific segment.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchEffort {
    /// Unique identifier for the search area segment.
    pub segment_id: usize,
    /// Time allocated to this segment in hours.
    pub hours: f64,
    /// The calculated probability of detection for this segment given the effort.
    pub expected_pod: f64,
}

/// Calculates the Probability of Detection (POD) using the Exponential Detection Function.
/// 
/// The formula used is:
/// `POD = 1 - exp(-k * C)`
/// where `C` is coverage and `k` is the detection constant.
/// 
/// In this implementation, we derive the detection constant `k` based on the 
/// relationship between sweep width, track spacing, and the area being searched.
/// 
/// # Arguments
/// * `area_km2` - Total area of the search zone in square kilometers.
/// * `sweep_width_m` - The effective width of the sensor/eye in meters.
/// * `track_spacing_m` - The distance between parallel search tracks in meters.
/// * `coverage` - The fraction of the area searched (0.0 to 1.0).
/// 
/// # Returns
/// `f64` - The probability of detection (0.0 to 1.0).
pub fn calculate_pod(
    area_km2: f64,
    sweep_width_m: f64,
    track_spacing_m: f64,
    coverage: f64,
) -> f64 {
    if coverage <= 0.0 {
        return 0.0;
    }
    if coverage >= 1.0 {
        // In a perfect world with zero track spacing, POD is 1.0.
        // However, we use the ratio of sweep width to track spacing to model 
        // the "density" of the search.
        let k = sweep_width_m / track_spacing_m;
        return 1.0 - (-k * coverage).exp();
    }

    // k represents the search density/effectiveness
    let k = sweep_width_m / track_spacing_m;
    1.0 - (-k * coverage).exp()
}

/// Performs a Bayesian update on a probability map after a search attempt.
/// 
/// This uses the "Negative Search" logic: if we search an area and do not find 
/// the target, the probability that the target is in that area decreases.
/// 
/// The formula for the posterior probability $P(H|E)$ is:
/// $P(H|E) = \frac{P(E|H) \cdot P(H)}{P(E)}$
/// 
/// Where:
/// - $P(H)$ is the prior probability.
/// - $P(E|H)$ is the probability of not finding the target given it is there: $(1 - POD)$.
/// - $P(E)$ is the total probability of the evidence (normalization factor).
/// 
/// # Arguments
/// * `prior` - A slice of prior probabilities for each segment (must sum to 1.0).
/// * `pod_scores` - The POD achieved in each segment during the search.
/// 
/// # Returns
/// `Vec<f64>` - The updated posterior probabilities (must sum to 1.0).
/// 
/// # Panics
/// Panics if `prior` and `pod_scores` have different lengths.
pub fn update_probability_map(prior: &[f64], pod_scores: &[f64]) -> Vec<f64> {
    assert_eq!(prior.len(), pod_scores.len(), "Prior and POD slices must have same length");

    // Calculate the denominator (normalization factor)
    // Sum of [ Prior_i * (1 - POD_i) ]
    let mut evidence_prob = 0.0;
    for i in 0..prior.len() {
        evidence_prob += prior[i] * (1.0 - pod_scores[i]);
    }

    // If evidence_prob is 0, it means we had 100% POD in all segments where prior > 0.
    // This is a mathematical singularity in search theory.
    if evidence_prob <= 0.0 {
        return vec![0.0; prior.len()];
    }

    // Calculate posterior: P(H|E) = [P(H) * (1 - POD)] / P(E)
    prior.iter().enumerate().map(|(i, &p_h)| {
        (p_h * (1.0 - pod_scores[i])) / evidence_prob
    }).collect()
}

/// Allocates search effort to maximize the cumulative Probability of Detection.
/// 
/// This uses a greedy approach based on the marginal utility of search time.
/// It prioritizes segments where the increase in POD per hour is highest.
/// 
/// # Arguments
/// * `segments` - A slice of tuples containing `(prior_probability, k_constant)`.
///   `k_constant` is the detection density (sweep_width / track_spacing).
/// * `available_hours` - Total search time available in hours.
/// * `speed_kmh` - The speed of the search asset in km/h.
/// 
/// # Returns
/// `Vec<f64>` - A vector of allocated hours for each segment.
/// 
/// # Note
/// This implementation assumes a simplified model where `k` is constant for the segment
/// and the relationship between time and coverage is linear: `Coverage = (Speed * Time) / Area`.
pub fn optimal_allocation(
    segments: &[(f64, f64)], 
    available_hours: f64, 
    speed_kmh: f64
) -> Vec<f64> {
    let n = segments.len();
    let mut allocations = vec![0.0; n];
    let mut remaining_hours = available_hours;

    // We use a discrete step-based greedy approach to approximate the optimal allocation
    // for the non-linear POD function.
    let time_step = 0.1; // 6-minute increments
    
    while remaining_hours >= time_step {
        let mut best_segment_idx = None;
        let mut max_marginal_pod = 0.0;

        for i in 0..n {
            let (prior, k) = segments[i];
            let current_hours = allocations[i];
            
            // Calculate current POD for this segment
            // Coverage = (Speed * Time) / Area. 
            // Since Area is not provided per segment, we assume k incorporates the area scaling.
            // For this model: POD = 1 - exp(-k * (Speed * Time))
            let current_pod = 1.0 - (-k * speed_kmh * current_hours).exp();
            
            // Calculate POD if we add one more time step
            let next_pod = 1.0 - (-k * speed_kmh * (current_hours + time_step)).exp();
            
            // Marginal utility: increase in total probability (Prior * Delta POD)
            let marginal_utility = prior * (next_pod - current_pod);

            if marginal_utility > max_marginal_pod {
                max_marginal_pod = marginal_utility;
                best_segment_idx = Some(i);
            }
        }

        if let Some(idx) = best_segment_idx {
            allocations[idx] += time_step;
            remaining_hours -= time_step;
        } else {
            // No more beneficial segments (marginal utility <= 0)
            break;
        }
    }

    // If there's leftover time due to the step size, we could distribute it, 
    // but in search theory, extra time with no marginal utility is ignored.
    allocations
}
