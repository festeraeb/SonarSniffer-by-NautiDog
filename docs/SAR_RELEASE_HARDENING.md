# CESAROPS — SAR Release Hardening & Alignment Notes

For the free search-and-rescue release. Focus: correctness where a wrong answer
sends a crew to the wrong place, and an honest account of the alignment stack.

## 1. The "20-mile drift" — real root cause and fix

**Root cause (per the system's designer):** a *linear* translation correction
applied to a *trigonometric* misregistration, compounded across a ~20-layer
temporal stack. Each layer has a slightly different rotation/projection skew; a
pure `(dx, dy)` shift cannot represent rotation, so the unmodelled rotational
residual accumulates nonlinearly across the stack into kilometre-scale drift.

**Fix (implemented):** `OverlayGrid::estimate_similarity` recovers a full 2-D
similarity transform — uniform scale `s`, rotation `θ`, translation `(tx, ty)` —
from the marker correspondences via the closed-form Umeyama/Procrustes
least-squares solution. The temporal stack (`scene_alignment_from_grid`) now uses
this instead of the translation-only mean. Because θ and s are fit directly, the
per-layer residual stays subpixel no matter how deep the stack. Verified:
`test_similarity_recovers_rotation_scale_translation` recovers a known
1.02×/3°/(+1.5,−0.8) transform to <0.05 px RMS; the translation-only path
provably cannot.

**Defence in depth (also in place):**
- Reject implausible fits: rotation > ±10° or scale outside ±10% ⇒ scene invalid.
- Gross-drift clamp: effective centre shift > 8 px (~80 m) ⇒ scene invalid.
- Inner/outer marker-set agreement ≤ 1 px required.
- A rejected scene keeps its raw geolocation (Sentinel-2 native ~5–10 m) rather
  than applying a corrupted correction — a reported position is never worse than
  the satellite's own accuracy.

**Recommended next:** snap the stamp origin to a fixed global grid (done in
`temporal.rs`) so every date stamps identical marker cells; and for processes
that warp pixels, re-detect markers in the *output* before fitting so the
transform is measured, not assumed.

## 2. Is NauticUVs used in the slice-and-stitch alignment?

**No — and this is an intentional, documented gap, not an oversight.**

- NauticUVs (the FDCT) IS used for *detection* — `curvelet_energy_ratio` scores
  structural energy in the mag and satellite candidate evaluators.
- The *registration* step (overlay grid) detects each fiducial marker by
  **intensity-weighted centroid**, not curvelet/shape correlation. So the
  alignment is brightness-based geometry, independent of nauticuvs.

Why this is fine for the SAR release:
- The centroid + dual-marker-set agreement + 8px clamp is robust enough that a
  bad alignment is *rejected*, not applied. Safety comes from the clamp, not
  from alignment cleverness.
- Keeping nauticuvs out of the public registration path also means the detuned
  public FDCT smear cannot degrade georeferencing.

Future enhancement (self-hosted / full-precision only): replace the centroid in
`marker_observation` with a curvelet shape-correlation — cross-correlate the
expected marker's geometric signature (which curvelets represent sparsely)
against the observed window. This would push alignment from ~1/8 px toward
~1/16 px and resist glint/clutter hijacking. Left out of the public build on
purpose.

## 3. SAR-release hardening checklist

Correctness (a wrong position is dangerous):
- [x] Gross-drift clamp on overlay alignment (≤8px or reject).
- [ ] Snap stamp origin to a fixed global grid (kills cause #3 at the source).
- [ ] Chip-bbox sanity assert: confirm the fetched chip's geo-bounds enclose the
      requested point before scoring (catches a swapped/duplicated COG).
- [ ] Round-trip a known wreck (e.g. Colgate 42.173, -81.740) through every
      pipeline in CI and assert the reported position is within 100 m.

Trust & reporting (SAR crews need calibrated confidence):
- [ ] Emit a per-candidate position-uncertainty radius (combine sensor
      geolocation + alignment confidence) so a crew sees a search circle, not a
      false-precision point.
- [ ] Always surface "raw vs corrected" coordinates and which was used.
- [ ] Precision/recall on the 47-wreck benchmark before publishing claims.

Safety & access (you control this via self-hosting):
- [ ] Move credentials.sh tokens to a secrets manager / env injection.
- [ ] Auth on the n8n webhooks before any non-VLAN exposure.
- [ ] Rate-limit / queue tasking so a SAR surge can't exhaust imagery quotas.

Operator UX (the adoption gate):
- [ ] One-call "search from last-known-position" workflow (backward drift →
      image the polygon → ranked candidates with search circles).
- [ ] Plain-language report ("3 anomalies in the search area; strongest is a
      120 m elongated target 2.1 km NE of LKP, confidence medium").

## 4. SonarSniffer (paid tier) — separation note

SonarSniffer stays a separate product for funding. Keep the boundary clean:
- The free SAR release ships satellite + aeromag + bag detection + drift +
  orchestration + the *public/detuned* nauticuvs.
- SonarSniffer (side-scan/RSD parsing, mosaic, the full-precision curvelet path)
  is the paid survey tool. No SAR-critical path should depend on it, so the free
  tier is fully functional standalone.

## 5. Dual-use boundary (recorded)

The platform is scoped to humanitarian SAR, survey, and civil infrastructure
monitoring. Adapting the detection chain to defeat military stealth/radar is out
of scope and not pursued. The detuned, 32-bit public release + self-hosted full
precision is the right access-control posture and is retained deliberately.
