## VERDICT: FAIL
## Score (0-10)
3/10

## Strengths
*   Includes the required "Cedarville-first" calibration logic.
*   Correctly identifies the BAG and Sentinel-2 cross-sensor relationship.
*   Includes the specific coordinate data and band requirements (B02, B03).

## Gaps / must-fix
*   **Structure Violation:** The output is highly repetitive. It provides a long narrative summary, followed by a bulleted list that repeats almost the exact same information.
*   **Constraint Violation:** The prompt requires "5 bullets." The model provided a long prose section followed by a 5-bullet list, rather than a concise 5-bullet report.
*   **Formatting Error:** The "GO" criteria section is truncated/broken (`data/straits_optical_clear|2`) and contains syntax errors.
*   **Technical Error:** The model identifies B04 and B08 as "thermal bands"; these are Red and Near-Infrared bands.
*   **Word Count/Efficiency:** While under 400 words, the redundancy makes the shard unusable for a pipeline handoff.

## Merge notes
Discard this shard. The model failed to follow the structural constraint (5 bullets) and instead produced a redundant, broken narrative. The technical error regarding spectral bands makes the data unreliable.