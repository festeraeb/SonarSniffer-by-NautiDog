# Ground-Truth Log

Verified outcomes from external sensors (side-scan, dive) used to label the ML
training set and correct the detectors. Each entry is a hard data point.

---

## 2026-06-03 — "Masked target / Robert Burns" (E of Elva) → GEOLOGY (ruled out)

- **Location:** ~45.8706–45.8719 N, -84.5871 to -84.5858 W (BAG H13255 mask013),
  ~0.4 nm SE of the Elva.
- **Detector said:** BAG uncertainty-unmask flagged an intact-looking cap,
  ~16 ft relief on the channel floor, interior relief, long axis ~129–150°.
- **Ground truth:** Deep View side-scan mosaic already covered the area.
  Verdict: **geology — parallel ridges, no wreckage.** Not a hull.
- **Label:** hard negative (`ml_label: 0`, type `geology_ridges`) in
  `scripts/known_wrecks_straits.json` and `data/known_wrecks_straits.json`.

### Why the detector was fooled (corrected discriminator)
- Interior relief alone is NOT a wreck discriminator — glacial ridge fields have
  interior relief too.
- **Floor-context test pointed the WRONG way:** an isolated cap on a smooth
  deepening thalweg looked wreck-like, but it was a ridge in a ridge field.
- **The true tell was azimuth:** all three nearby features (mask013/194/079)
  aligned with the channel/ice-flow bearing (~138°) within 25°. **Parallel +
  repeating + channel-aligned = geology.** A single ISOLATED, OFF-axis cap is
  the wreck signature; an on-axis member of a parallel set is not.
- Current-aligned-wreck argument is real but must not override the
  "parallel/repeating set" signal: if ≥2 similar features share the channel
  axis and spacing, treat the family as geology pending another sensor.

### Action items
- BAG unmask: down-weight candidates that are (a) channel/thalweg-aligned AND
  (b) members of a parallel, similarly-spaced ridge set. Flag the lone off-axis
  outlier instead.
- ML: add mask013/194/079 features as hard negatives (interior-relief + L:B +
  channel-axis-delta + parallel-neighbor-count) — this is the exact
  false-positive class the classifier must learn to reject.

### Tooling produced (kept in repo root)
- `find_mask.py` — locate a BAG mask tile by lat/lon.
- `pin_on_mask.py` — render reconstruction + pin points/footprint boxes.
- `profile_target.py` — interior-relief / geometry profiler.
- `isolate_object.py` — detrend seabed, flood-fill + measure the raised object.
- `channel_floor.py` — floor-depth sampling + down-channel transect.
- `axis_azimuth.py` — long-axis azimuth vs channel bearing (the decisive test).
