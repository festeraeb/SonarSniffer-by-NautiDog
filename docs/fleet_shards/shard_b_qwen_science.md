

<!-- reasoning -->
Here's a thinking process:

1.  **Analyze User Input:**
   - **Role:** Scientific reviewer for Straits wreck satellite search.
   - **Emphasis:** Calibration discipline and physics.
   - **Input:** A detailed handoff spec for a satellite pipeline (`cesarops-satellite`) searching for a masked wreck (Robert Burns) and calibrating against a known wreck (Cedarville) using Sentinel-2 optical data.
   - **Key Constraints/Physics:**
     - Target depth ~34m (beyond SDB bottom-detection limit of ~20-30m). Signal is water-column disturbance/plume, not bottom reflectance.
     - Cedarville (steel) = strong thermal/cold-sink signal. Burns (wood) = weak thermal, relies on physical obstruction (zebra-clarity, glint modulation, shadow/texture).
     - Bands: B02 (blue) + B03 (green) for water penetration. B04/B08 for surface only.
     - Glint/currents = signal, not noise.
     - Calibration order: Cedarville first, then controls, then Burns AOI.
     - Must deliver: Calibration order, Concept weights table, False confidence guards, What would falsify a Burns hit.
     - Length: Under 600 words.
     - Tone: Scientific reviewer, emphasizing calibration discipline and physics.

2.  **Deconstruct Requirements:**
   - **Calibration order:** Cedarville → controls → Burns AOI. Need to justify why and how.
   - **Concept weights:** Table mapping concepts to wreck types (steel vs wood) and bands.
   - **False confidence guards:** Mechanisms to prevent spurious hits (e.g., cloud/shadow artifacts, glint misinterpretation, depth mismatch, temporal instability).
   - **What would falsify a Burns hit:** Specific physical/observational criteria that would invalidate a candidate.
   - **Constraints:** Under 600 words. Emphasize calibration discipline and physics.

3.  **Draft - Section by Section (Mental Refinement):**
   *(Calibration Order)*
   1. Cedarville (steel, shallow, known geometry) → validates thermal/cold-sink + column clarity concepts. Must yield a candidate within 300m with composite_score > threshold.
   2. Shallow controls (Eber Ward, William Young, M. Stalker, Elva) → validate band selection (B02/B03 penetration) and zebra-clarity/glint concepts on known wood/steel mix.
   3. Burns AOI (45.87127°N, -84.58642°W) → only after 1 & 2 pass. Multi-date stack required to confirm advected plume coherence.

   *(Concept Weights Table)*
   Need a clear table mapping concept, band dependency, steel weight, wood weight, physics basis.
   - `thermal_anomaly`: B02/B03 (proxy), Steel: HIGH, Wood: LOW. Physics: Heat sink vs insulation.
   - `zebra_clarity`: B02+B03, Steel: MED, Wood: HIGH. Physics: Physical obstruction modulating subsurface light field.
   - `glint_roughness`: B02/B03 surface, Steel: MED, Wood: HIGH. Physics: Current-driven surface roughness modulation over relief.
   - `temporal_plume`: Multi-date B02/B03, Steel: HIGH, Wood: MED. Physics: Advected turbidity/clarity plume coherence across dates.

   *(False Confidence Guards)*
   - Depth mismatch: Candidate depth <20m or >40m → reject (SDB limit).
   - Temporal instability: Signal present in <3 dates → reject (likely cloud/shadow/artifact).
   - Band cross-talk: Over-reliance on B04/B08 → reject (surface-only, no column penetration).
   - Glint saturation: High solar zenith angle + specular peak → flag as false positive; require low-glint or cross-date consistency.
   - Spatial coherence: Candidate footprint <50m or >200m → reject (mismatch with 110-130ft × 30-40ft beam).

   *(What Would Falsify a Burns Hit)*
   - Absence of multi-date coherence (plume doesn't advect consistently).
   - Signal vanishes when glint is minimized (low sun angle) → indicates surface specular artifact, not column obstruction.
   - Depth proxy >35m or <25m → violates SDB physics for this water clarity.
   - No correlation with known current vectors → advected plume direction contradicts Straits hydrodynamics.
   - Cedarville fails calibration → pipeline concepts are misweighted; all subsequent hits are untrustworthy.

4.  **Check Constraints & Refine (Word Count & Tone):**
   Let's count words and ensure it's under 600. I'll structure it clearly as requested.
   I need to make sure the tone is strictly scientific, emphasizing calibration discipline and physics. I'll tighten the language.

   *Draft Generation:*
   **Calibration Order**
   1. **Cedarville (Steel/Thermal):** Mandatory first pass. Validates cold-sink thermal proxy, column clarity, and multi-date plume coherence. Candidate must fall within 300m of 45.7873°N, -84.6708°W with composite_score exceeding the calibrated threshold. Failure here invalid