# Triple-Lock Detection Pipeline — Validation Coordinator

## Architecture

The Triple-Lock eliminates false positives by requiring three independent confirmations
before classifying an anomaly as a wreck candidate. A sandbar might fool one vision model,
but it won't have the thermal oscillation pattern of a localized steel mass.

```
GeoTIFF Tile
    ↓
[SCOUT] 1060 (Florence-2) — Visual anomaly detection
    ↓ anomaly detected?
[CROSS] P1000 (Moondream2) — Independent visual validation
    ↓ both agree?
[JITTER] TPU VM (TFLite) — Thermal time-series physics check
    ↓ metal/density profile matches?
[REASON] 1070 (DeepSeek-R1) — Final COT reasoning + classification
    ↓
Mission Action: CONFIRM / INVESTIGATE / STANDBY
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

## TPU Jitter Handshake (Host ↔ VM)

The TPU lives in a KVM VM (Ubuntu 22.04, kernel 6.8, gasket driver working).
Communication via internal bridge network.

**Host Request:** Sends raw thermal time-series array to VM's internal IP (e.g., 192.168.122.x:8090)
**VM Logic:** Runs TFLite model on passed-through Coral PCIe Edge TPU
**Result:** Returns JSON:
```json
{
    "material": "iron_cargo",
    "certainty": 0.89,
    "depth_estimate": 512,
    "thermal_oscillation_hz": 0.003,
    "mass_estimate_kg": 45000
}
```

## Mission Control UI — Three Status Lights

For every processed tile, Mission Control displays:

```
[ SCOUT  ] (1060) — 🟢 Green if anomaly detected
[ CROSS  ] (P1000) — 🟢 Green if visually confirmed  
[ JITTER ] (TPU) — 🟢 Green if metal/density profile matches
```

All three green = **VALIDATED** wreck candidate
Two green = **HIGH** confidence, needs deeper analysis
One green = **LOW** confidence, likely false positive

## Why This Eliminates False Positives

- **Sandbars**: Fool optical (1060 + P1000) but have NO thermal jitter signature → TPU rejects
- **Boat wakes**: Transient, disappear in temporal stack → Scout rejects on second pass
- **Natural geology**: May have thermal signature but wrong geometry → Cross-validation rejects
- **Steel wreck at depth**: Creates predictable thermal oscillation from heat sink effect, 
  has linear geometry visible in optical, and produces consistent jitter pattern → ALL THREE CONFIRM

The rail iron detection at 500ft depth is the proof case: the thermal cold disturbance
jitter pattern is distinctive enough to identify cargo type. No sandbar or geological
feature produces that signature.

## Hardware Assignment

| Node | GPU | Model | Role | Latency |
|------|-----|-------|------|---------|
| cesarops3 | GTX 1060 6GB | Florence-2-large | Primary scout | ~50ms/patch |
| cesarops2 | Quadro P1000 4GB | Moondream2 Q4 | Cross-validator | ~100ms/patch |
| T440 VM | Coral Edge TPU | TFLite jitter model | Physics check | ~5ms/inference |
| cesarops2 | GTX 1070 8GB | DeepSeek-R1 8B | Final reasoning | ~2s/decision |

## Integration Points

- Scout + Validator communicate via HTTP (OpenAI-compatible vision endpoints)
- TPU VM exposed via internal bridge at 192.168.122.x:8090
- Reasoner (1070) receives structured findings from all three, applies COT
- Results flow to Mission Control via WebSocket for real-time status lights
- Confirmed detections → GitHub issue via GitHub_Agent
- All findings indexed into nautivecs for future reference
