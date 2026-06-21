use crate::error::CurveletError;
#[derive(Debug, Clone)]
pub struct CurveletConfig {
    pub num_scales: usize,
    pub finest_scale_directions: usize,
    pub directions_per_scale: Option<Vec<usize>>,
    pub(crate) original_rows: usize,
    pub(crate) original_cols: usize,
    pub(crate) padded_size: usize,
}
impl CurveletConfig {
    pub fn new(num_scales: usize) -> Result<Self, CurveletError> {
        if !(2..=10).contains(&num_scales) {
            return Err(CurveletError::InvalidScaleCount(num_scales));
        }
        Ok(Self {
            num_scales,
            finest_scale_directions: 32,
            directions_per_scale: None,
            original_rows: 0,
            original_cols: 0,
            padded_size: 0,
        })
    }
    pub fn with_finest_directions(mut self, d: usize) -> Result<Self, CurveletError> {
        if d < 4 || !d.is_multiple_of(4) {
            return Err(CurveletError::InvalidDirectionCount(d));
        }
        self.finest_scale_directions = d;
        Ok(self)
    }
    pub fn with_directions_per_scale(mut self, dirs: Vec<usize>) -> Result<Self, CurveletError> {
        let num_detail = self.num_detail_scales();
        if dirs.len() != num_detail {
            return Err(CurveletError::DirectionCountMismatch {
                expected: num_detail,
                got: dirs.len(),
            });
        }
        for &d in &dirs {
            if d < 4 || !d.is_multiple_of(4) {
                return Err(CurveletError::InvalidDirectionCount(d));
            }
        }
        self.directions_per_scale = Some(dirs);
        Ok(self)
    }
    #[inline]
    pub fn num_detail_scales(&self) -> usize {
        self.num_scales.saturating_sub(2)
    }
    pub fn directions_at_detail_scale(&self, detail_idx: usize) -> usize {
        if let Some(ref dirs) = self.directions_per_scale {
            return dirs[detail_idx];
        }
        let n_detail = self.num_detail_scales();
        if n_detail == 0 {
            return 0;
        }
        let finest_idx = n_detail - 1;
        let levels_from_finest = finest_idx - detail_idx;
        let halvings = levels_from_finest / 2;
        let dirs = self.finest_scale_directions >> halvings;
        dirs.max(8)
    }
}