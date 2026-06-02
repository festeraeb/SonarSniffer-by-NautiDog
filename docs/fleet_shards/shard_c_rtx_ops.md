

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Context:** Handoff from a previous agent for running `cesarops-satellite` (`sat-run`) on local Sentinel-2 tiles to detect a masked wreck (Robert Burns) and calibrate on a known wreck (Cedarville).
   - **Mission:** Run `sat-run` with local offline data, parallelized with rayon, on P100 GPUs (optional later). Focus on STEPS 3-4 + ACCEPTANCE.
   - **Key Targets:**
     - Cedarville: 45.7873°N, -84.6708°W (calibration target)
     - Robert Burns (masked target): 45.87127°N, -84.58642°W
     - Distance threshold: ~300m
   - **Data Paths:** `data