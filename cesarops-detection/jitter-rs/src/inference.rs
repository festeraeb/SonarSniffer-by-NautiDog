//! Primary inference backend.
//!
//! Loads an ONNX jitter model with `tract` (pure-Rust, CPU) when `JITTER_MODEL`
//! points at a readable `.onnx` file. Otherwise falls back to the deterministic
//! thermal heuristic. The `gpu` feature reserves a wgpu compute path.

use crate::heuristic;
use crate::types::{Candidate, JitterRequest};
use std::path::Path;
use std::sync::Arc;
use tracing::{info, warn};

use tract_onnx::prelude::*;
use anyhow::anyhow;

type Model = RunnableModel<TypedFact, Box<dyn TypedOp>>;

#[derive(Clone)]
pub struct Engine {
    model: Option<Arc<Model>>,
    backend: String,
}

impl Engine {
    /// Build the engine, loading a model from `JITTER_MODEL` if available.
    pub fn init() -> Self {
        let model_path = std::env::var("JITTER_MODEL").unwrap_or_default();
        if model_path.is_empty() || !Path::new(&model_path).is_file() {
            info!("no JITTER_MODEL — primary backend = cpu_thermal heuristic");
            return Self {
                model: None,
                backend: "cpu_thermal".to_string(),
            };
        }

        match Self::load_onnx(&model_path) {
            Ok(m) => {
                let backend = if cfg!(feature = "gpu") {
                    "tract_gpu"
                } else {
                    "tract_cpu"
                };
                info!("loaded ONNX jitter model on {backend}: {model_path}");
                Self {
                    model: Some(m),
                    backend: backend.to_string(),
                }
            }
            Err(e) => {
                warn!("failed to load ONNX model ({e}); falling back to heuristic");
                Self {
                    model: None,
                    backend: "cpu_thermal".to_string(),
                }
            }
        }
    }

    fn load_onnx(path: &str) -> TractResult<Arc<Model>> {
        // Input: [1, N] thermal feature vector. We declare a flexible 8-wide
        // feature so a typical small jitter MLP loads; real models can override
        // via their own declared shapes.
        let model = tract_onnx::onnx()
            .model_for_path(path)?
            .into_optimized()?
            .into_runnable()?;
        Ok(model)
    }

    pub fn backend(&self) -> &str {
        &self.backend
    }

    /// Produce the primary candidate for a tile.
    pub fn infer(&self, req: &JitterRequest) -> Candidate {
        match &self.model {
            None => heuristic::evaluate(req),
            Some(model) => self.infer_onnx(req, model).unwrap_or_else(|e| {
                warn!("ONNX inference error ({e}); using heuristic for {}", req.tile_id);
                heuristic::evaluate(req)
            }),
        }
    }

    fn infer_onnx(&self, req: &JitterRequest, model: &Arc<Model>) -> TractResult<Candidate> {
        let feats = featurize(req);
        let input = tract_ndarray::Array2::from_shape_vec((1, feats.len()), feats)?;
        let tensor: Tensor = input.into();
        let result = model.run(tvec!(tensor.into()))?;

        // Expect a single output: either [1,1] certainty or [1,2] logits.
        let out_tensor = result[0].clone().into_tensor();
        let view = out_tensor.to_plain_array_view::<f32>()?;
        let out: &[f32] = view.as_slice().ok_or_else(|| anyhow!("non-contiguous output"))?;
        let certainty = match out.len() {
            1 => out[0] as f64,
            n if n >= 2 => softmax2(out[0], out[1]) as f64,
            _ => return Err(anyhow!("unexpected output len {}", out.len())),
        }
        .clamp(0.0, 0.99);

        // Reuse heuristic frequency/thermal envelope (model only scores certainty).
        let base = heuristic::evaluate(req);
        let material = if certainty > 0.7 {
            "ferrous_composite"
        } else {
            "natural"
        };
        Ok(Candidate {
            material: material.to_string(),
            certainty: heuristic::round3(certainty),
            jitter_frequency_hz: base.jitter_frequency_hz,
            thermal_delta_c: base.thermal_delta_c,
            backend: self.backend.clone(),
        })
    }
}

/// Build a fixed-width feature vector from the request.
fn featurize(req: &JitterRequest) -> Vec<f32> {
    let n = req.thermal_timeseries.len() as f32;
    let lat = req.coordinates.lat as f32;
    let lon = req.coordinates.lon as f32;
    let depth = req.depth_estimate_m as f32;
    vec![
        n,
        n.min(6.0),
        lat / 90.0,
        lon / 180.0,
        depth / 500.0,
        (depth > 100.0) as i32 as f32,
        (n >= 2.0) as i32 as f32,
        1.0, // bias
    ]
}

fn softmax2(a: f32, b: f32) -> f32 {
    let m = a.max(b);
    let ea = (a - m).exp();
    let eb = (b - m).exp();
    eb / (ea + eb)
}
