//! Rolling Drift Statistics — circular buffer with outlier filtering.
//!
//! Memory-efficient (fixed size, no growing vectors).
//! Filters out the single highest outlier to prevent "creativity spikes"
//! from triggering false-positive retries on exploratory segments.

/// Circular buffer for rolling drift averages
pub struct RollingDrift {
    window: Vec<f64>,
    capacity: usize,
    pointer: usize,
}

impl RollingDrift {
    pub fn new(capacity: usize) -> Self {
        Self {
            window: Vec::with_capacity(capacity),
            capacity,
            pointer: 0,
        }
    }

    /// Push a new drift score into the circular buffer
    pub fn push(&mut self, score: f64) {
        if self.window.len() < self.capacity {
            self.window.push(score);
        } else {
            self.window[self.pointer] = score;
            self.pointer = (self.pointer + 1) % self.capacity;
        }
    }

    /// Filtered average: ignores the single highest outlier.
    /// This prevents one creative/exploratory segment from spiking the average
    /// and triggering unnecessary retries on subsequent precision segments.
    pub fn filtered_average(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        if self.window.len() < 3 {
            // Not enough data to filter — return raw average
            return self.window.iter().sum::<f64>() / self.window.len() as f64;
        }

        let mut sorted = self.window.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        // Remove the highest outlier (the creativity spike)
        let sum: f64 = sorted.iter().take(sorted.len() - 1).sum();
        sum / (sorted.len() - 1) as f64
    }

    /// Raw average without filtering (for diagnostics)
    pub fn raw_average(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        self.window.iter().sum::<f64>() / self.window.len() as f64
    }

    /// How many samples are in the buffer
    pub fn len(&self) -> usize {
        self.window.len()
    }

    /// Is the buffer empty
    pub fn is_empty(&self) -> bool {
        self.window.is_empty()
    }

    /// Is the buffer full (at capacity)
    pub fn is_full(&self) -> bool {
        self.window.len() >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_returns_zero() {
        let rd = RollingDrift::new(5);
        assert_eq!(rd.filtered_average(), 0.0);
    }

    #[test]
    fn test_single_value() {
        let mut rd = RollingDrift::new(5);
        rd.push(0.03);
        assert_eq!(rd.filtered_average(), 0.03);
    }

    #[test]
    fn test_outlier_filtered() {
        let mut rd = RollingDrift::new(5);
        rd.push(0.02);
        rd.push(0.03);
        rd.push(0.02);
        rd.push(0.14); // creativity spike — should be filtered
        rd.push(0.03);

        let avg = rd.filtered_average();
        // Without filter: (0.02+0.03+0.02+0.14+0.03)/5 = 0.048
        // With filter (drop 0.14): (0.02+0.03+0.02+0.03)/4 = 0.025
        assert!(avg < 0.03, "Filtered average should be ~0.025, got {}", avg);
    }

    #[test]
    fn test_circular_overwrites() {
        let mut rd = RollingDrift::new(3);
        rd.push(0.01);
        rd.push(0.02);
        rd.push(0.03);
        rd.push(0.04); // overwrites 0.01

        assert_eq!(rd.len(), 3);
        assert!(rd.is_full());
        // Window should be [0.04, 0.02, 0.03] (pointer wrapped)
        let avg = rd.raw_average();
        assert!((avg - 0.03).abs() < 0.001);
    }
}

/// Weighted variant — stores (score, weight) pairs.
/// Exploratory segments contribute less to the average (weight=0.3).
/// More precise than outlier filtering for mixed-mode pipelines.
pub struct WeightedRollingDrift {
    history: Vec<(f64, f64)>,
    capacity: usize,
}

impl WeightedRollingDrift {
    pub fn new(capacity: usize) -> Self {
        Self {
            history: Vec::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, score: f64, weight: f64) {
        if self.history.len() >= self.capacity {
            self.history.remove(0);
        }
        self.history.push((score, weight));
    }

    /// Weighted average: each score contributes proportional to its mode weight
    pub fn weighted_average(&self) -> f64 {
        let total_weight: f64 = self.history.iter().map(|(_, w)| w).sum();
        if total_weight == 0.0 {
            return 0.0;
        }
        self.history.iter().map(|(s, w)| s * w).sum::<f64>() / total_weight
    }

    pub fn len(&self) -> usize {
        self.history.len()
    }

    pub fn is_empty(&self) -> bool {
        self.history.is_empty()
    }
}

#[cfg(test)]
mod weighted_tests {
    use super::*;

    #[test]
    fn test_weighted_exploratory_contributes_less() {
        let mut wd = WeightedRollingDrift::new(5);
        wd.push(0.02, 1.0); // precision
        wd.push(0.03, 1.0); // precision
        wd.push(0.14, 0.3); // exploratory — high score but low weight

        let avg = wd.weighted_average();
        // Without weighting: (0.02+0.03+0.14)/3 = 0.063
        // With weighting: (0.02*1 + 0.03*1 + 0.14*0.3) / (1+1+0.3) = 0.092/2.3 = 0.040
        assert!(avg < 0.05, "Weighted avg should be ~0.04, got {}", avg);
    }
}
