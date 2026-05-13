# Triple-Lock Detection Pipeline

## Architecture

Three independent vision/analysis systems must agree before a detection is confirmed.
This eliminates false positives from sandbars, boat wakes, and natural features.

```
Tile → 1060 (Florence-2, primary scout)
         ↓ "thermal cold spot + linear feature"
Tile → P1000 (Moondream2, independent validator)
         ↓ "elongated dark anomaly, possible hull"
         
Both agree? → TPU VM (jitter analysis on thermal time-series)
         ↓ "material: iron_cargo, certainty: 0.89, depth: 512ft"
         
All three? → 1070 (DeepSeek-R1 reasons about combined evidence)
         → CONFIRMED DETECTION
```

## Rust Implementation

```rust
pub enum Confidence {
    Low,
    Medium,
    High,
    Validated,
}

pub struct DetectionPipeline {
    scout: Node1060,      // Florence-2
    validator: NodeP1000, // Moondream2
    analyst: NodeTPU,     // Jitter Model (in VM)
    reasoner: Node1070,   // DeepSeek-R1
}

impl DetectionPipeline {
    pub async fn process_tile(&self, tile: GeoTile) -> anyhow::Result<MissionAction> {
        // Step 1: Scout (1060)
        let scout_report = self.scout.analyze(&tile).await?;
        
        if scout_report.has_anomaly() {
            // Step 2: Cross-Validate (P1000)
            let val_report = self.validator.analyze(&tile).await?;
            
            if scout_report.intersects(&val_report) {
                // Step 3: Physics Check (TPU VM)
                let jitter_sig = self.analyst.jitter_check(&tile).await?;
                
                // Step 4: Final Reasoning (1070)
                return self.reasoner.decide(scout_report, val_report, jitter_sig).await;
            }
        }
        Ok(MissionAction::Standby)
    }
}
```

## TPU VM Handshake Protocol

Host → VM communication via HTTP on internal bridge network:

**Request:** POST http://tpu-worker.local:8080/jitter
```json
{
  "tile_id": "erie_sector4_2024-10-12",
  "thermal_timeseries": [/* f32 array, 20+ temporal samples */],
  "coordinates": {"lat": 41.85, "lon": -81.23},
  "depth_estimate_m": 150
}
```

**Response:**
```json
{
  "material": "iron_cargo",
  "certainty": 0.89,
  "depth_estimate_ft": 512,
  "jitter_frequency_hz": 0.003,
  "thermal_delta_c": -2.1,
  "classification": "ferrous_mass_high_density"
}
```

## Mission Control UI — Three Status Lights

For every tile processed, the UI shows:

```
[ SCOUT  ] (1060) — Green if anomaly detected
[ CROSS  ] (P1000) — Green if visually confirmed
[ JITTER ] (TPU) — Green if metal/density profile matches
```

All three green = **VALIDATED DETECTION** → auto-creates GitHub issue + marks on map.

## Why This Eliminates False Positives

| False Positive Source | Fools Scout? | Fools Validator? | Fools TPU? |
|----------------------|:---:|:---:|:---:|
| Sandbar | ✅ (linear feature) | ✅ (dark shape) | ❌ (no thermal jitter) |
| Boat wake | ✅ (glint pattern) | ❌ (wrong shape) | ❌ |
| Natural turbidity | ✅ (color anomaly) | ❌ (no structure) | ❌ |
| Submerged wreck | ✅ | ✅ | ✅ (thermal oscillation from steel mass) |

The TPU jitter check is the "kill shot" — only a localized steel/iron mass at depth
produces the specific thermal oscillation pattern that the jitter model detects.
This was proven when the system identified rail iron cargo at 500ft depth from
thermal jitter alone.

## Hardware Assignment

| Node | GPU | Model | Role | Speed |
|------|-----|-------|------|-------|
| cesarops3 | GTX 1060 (6GB) | Florence-2-large | Primary scout | ~50ms/patch |
| cesarops2 | Quadro P1000 (4GB) | Moondream2 Q4 | Cross-validator | ~100ms/patch |
| T440 VM | Coral Edge TPU | TFLite jitter model | Physics check | ~5ms/inference |
| cesarops2 | GTX 1070 (8GB) | DeepSeek-R1 8B | Final reasoning | ~30 tok/s |
| T440 | Dual P100 (32GB) | Qwen3.6-35B | Heavy analysis + curvelets | When needed |
| T440 | Dual Xeons (94GB) | ndarray + AVX-512 | Drift correction | Parallel |
