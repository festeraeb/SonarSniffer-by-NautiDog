## Wreck candidates (ranked)

1. **Candidate 1:**
   - **Tile ID:** Holloway-probe-42N83W
   - **Coordinates:** Latitude 42.331, Longitude -83.048
   - **Detection Score:** 0.298
   - **Material Classification:** Ferrous Composite
   - **Depth Estimate:** 137.8 feet
   - **Certainty:** 0.738
   - **Classification:** Confirmed Structure
   - **Notes:** Detected via Movidius jitter with a high certainty and confirmed as a structure.

## Cue matrix (glint / dark / clear water / jitter / ripple-same-spot)

| Glint | Dark | Clear Water | Jitter | Ripple-Same-Spot |
|-------|------|-------------|--------|------------------|
| 0.298 | 0    | 1           | 0.738  | 1                |

## False-positive guards

1. **Low Detection Score:** Any detection with a score below 0.3 should be flagged for further review.
2. **Uncertain Material Classification:** Any classification with a certainty below 0.7 should be re-verified.
3. **Non-Structural Classification:** Any classification that does not identify a structure should be disregarded.
4. **Depth Anomalies:** Any depth estimate that deviates significantly from the expected range for the survey area should be investigated.

## Code sketch (file paths + process_tile logic)

### File Paths

- **Detection Module:** `src/detection/detection_module.py`
- **Classification Module:** `src/classification/classification_module.py`
- **Tile Processing Module:** `src/tile_processing/tile_processor.py`
- **Data Storage Module:** `src/data_storage/data_storage_module.py`

### process_tile() Logic

```python
def process_tile(tile_data):
    """
    Process a single tile of satellite data to identify potential wreck candidates.

    Args:
    tile_data (dict): The tile data containing detection and classification information.

    Returns:
    list: A list of ranked wreck candidates.
    """
    # Extract relevant data
    tile_id = tile_data["tile_id"]
    coordinates = tile_data["coordinates"]
    tpu_scan = tile_data["tpu_scan"]
    movidius_jitter = tile_data["movidius_jitter"]

    # Initialize candidate list
    candidates = []

    # Check for TPU detection
    if tpu_scan["detection_count"] > 0:
        for detection in tpu_scan["top_detections"]:
            if detection["score"] >= 0.3:
                # Check for Movidius jitter classification
                if movidius_jitter["certainty"] >= 0.7 and movidius_jitter["classification"] == "confirmed_structure":
                    candidate = {
                        "tile_id": tile_id,
                        "coordinates": coordinates,
                        "detection_score": detection["score"],
                        "material_classification": movidius_jitter["material"],
                        "depth_estimate_ft": movidius_jitter["depth_estimate_ft"],
                        "certainty": movidius_jitter["certainty"],
                        "classification": movidius_jitter["classification"]
                    }
                    candidates.append(candidate)

    # Rank candidates based on detection score and certainty
    candidates.sort(key=lambda x: (x["detection_score"], x["certainty"]), reverse=True)

    # Store ranked candidates
    data_storage_module.store_candidates(candidates)

    return candidates
```

This code sketch outlines the logic for processing a tile of satellite data to identify potential wreck candidates, rank them based on detection score and certainty, and store the results for further analysis.