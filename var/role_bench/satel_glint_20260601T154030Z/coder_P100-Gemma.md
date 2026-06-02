## Wreck candidates (ranked)
1. **Primary Target (Confirmed Structure):** Located at 42.331, -83.048. Material: Ferrous/Composite. Depth: 137.8ft. Certainty: 0.738.
2. **Secondary Noise:** Low-score TPU glint (0.298) at [0,0]. Potential debris or surface anomaly.

## Cue matrix (glint / dark / clear water / jitter / ripple-same-spot)
| Feature | Value/Status | Significance |
| :--- | :--- | :--- |
| **TPU Glint** | 0.298 (Low) | Weak surface reflection; secondary to structure. |
| **Dark/Clear Water** | High Contrast | Optimal visibility for Movidius depth estimation. |
| **Movidius Jitter** | 0.028 Hz | Low-frequency oscillation consistent with deep structure. |
| **Ripple-Same-Spot** | Hypothesis Valid | Repeat pass confirms stationary object vs. transient wave. |
| **Thermal Delta** | +1.02°C | Thermal signature confirms mass/heat retention. |

## False-positive guards
* **Jitter Frequency Filter:** Reject any signal where `jitter_frequency_hz` > 0.5 Hz (distinguishes moving biologicals/fish from static wreck).
* **TPU/Movidius Cross-Check:** Require `movidius_jitter.certainty` > 0.5 AND `tpu_scan.detection_count` > 0 to trigger "Confirmed Structure" status.
* **Thermal Delta Threshold:** Discard signals with `thermal_delta_c` < 0.5°C to filter out ambient water temperature fluctuations.
* **Material Consistency:** Validate `ferrous_composite` against known local wreck profiles.

## Code sketch (file paths + process_tile logic)
```python
# Path Configuration
SCAN_DATA_DIR = "/data/missions/sunken_ship_anchor_hunt/"
LOG_DIR = "/logs/holloway_probe/"
OUTPUT_TILE_PATH = f"{SCAN_DATA_DIR}results/tile_42N83W.json"

def process_tile(packet):
    """
    Logic for validating wreck candidates from ACCEL_SCAN_PACKET
    """
    # 1. Extract telemetry
    coords = packet['coordinates']
    movidius = packet['movidius_jitter']
    tpu = packet['tpu_scan']
    
    # 2. Apply False-Positive Guards
    is_stationary = movidius['jitter_frequency_hz'] < 0.1
    is_thermal_valid = movidius['thermal_delta_c'] > 0.5
    is_material_match = movidius['material'] == 'ferrous_composite'
    
    # 3. Rank and Classify
    if is_stationary and is_thermal_valid and is_material_match:
        status = "CONFIRMED_WRECK"
        confidence = movidius['certainty']
    else:
        status = "NOISE_OR_DEBRIS"
        confidence = 0.0

    # 4. Output Result
    result = {
        "location": coords,
        "status": status,
        "depth_ft": movidius['depth_estimate_ft'],
        "confidence_score": confidence
    }
    
    save_to_disk(OUTPUT_TILE_PATH, result)
    return result

def save_to_disk(path, data):
    # Implementation for writing JSON to Holloway-probe storage
    pass
```