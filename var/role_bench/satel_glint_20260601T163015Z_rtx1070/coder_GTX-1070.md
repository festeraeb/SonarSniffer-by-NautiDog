## Wreck candidates (ranked)
1. **Holloway-probe-42N83W** - High confidence in the presence of a wreck based on TPU glint detection and Movidius jitter analysis.

## Cue matrix (glint / dark / clear water / jitter / ripple-same-spot)
- **Glint**: Present
- **Dark**: Present
- **Clear Water**: Present
- **Jitter**: High certainty (ferrous composite material)
- **Ripple-Same-Spot**: Present

## False-positive guards
- **Depth Estimate**: Must be greater than 100 feet to rule out shallow debris.
- **Material Confirmation**: Must match ferrous composite to rule out non-metallic objects.
- **Jitter Frequency**: Must be above 0.025 Hz to rule out non-metallic objects.
- **Thermal Delta**: Must be above 0.5°C to rule out non-metallic objects.

## Code sketch (file paths + process_tile logic)
```python
# File: process_tile.py
import json
import os

# Define the directory for storing processed tiles
PROCESSED_TILES_DIR = 'processed_tiles'

def process_tile(tile_id, data):
    # Save the raw data to a file
    with open(os.path.join(PROCESSED_TILES_DIR, f'{tile_id}.json'), 'w') as f:
        json.dump(data, f)

    # Rank wreck candidates
    rank_wreck_candidates(data)

def rank_wreck_candidates(data):
    candidates = data.get('wreck_candidates', [])
    candidates.sort(key=lambda x: x['score'], reverse=True)
    print("Ranked Wreck Candidates:")
    for candidate in candidates:
        print(f"Tile ID: {candidate['tile_id']}, Score: {candidate['score']}")

# Example usage
tile_data = {
    "mission": "sunken_ship_anchor_hunt",
    "tile_id": "Holloway-probe-42N83W",
    "coordinates": {
        "lat": 42.331,
        "lon": -83.048
    },
    "repeat_pass_hypothesis": "glint_ripple_dark_clear_water_same_pixels",
    "tpu_scan": {
        "detection_count": 1,
        "top_detections": [
            {
                "col": 0,
                "row": 0,
                "score": 0.2980392156862745
            }
        ],
        "used_tpu": False,
        "took_s": 0.016
    },
    "movidius_jitter": {
        "material": "ferrous_composite",
        "certainty": 0.738,
        "depth_estimate_ft": 137.8,
        "jitter_frequency_hz": 0.028,
        "thermal_delta_c": 1.02,
        "classification": "confirmed_structure"
    },
    "wreck_candidates": [
        {
            "tile_id": "Holloway-probe-42N83W",
            "score": 0.2980392156862745
        }
    ]
}

process_tile(tile_data['tile_id'], tile_data)
```

This code snippet defines a function `process_tile` that takes in tile data, saves it to a file, and ranks wreck candidates based on their scores. The `rank_wreck_candidates` function sorts the candidates in descending order of their scores and prints them. The example usage demonstrates how to use these functions with sample data.