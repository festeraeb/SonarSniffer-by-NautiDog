# Requirements Document

## Introduction

The `nauticuvs` precision engine is a foundational upgrade to the `nauticuvs` Rust crate — the
mathematical core shared by `cesarops-aeromagnetic-worker`, `cesarops-satellite-worker`, and
`sovereign-cloud`. The crate currently exposes a `curvelet_forward` function but lacks a concrete
in-tree implementation, has no coordinate-reference-system (CRS) awareness, operates exclusively
in f32, and stores no phase information in curvelet coefficients.

This feature delivers eight capabilities across three phases:

- **Phase 1 — Precision & Data Integrity**: selectable f32/f64 floating-point precision, SIMD-aligned
  `Subband` memory layout, and CRS-preserving GeoTIFF ingestion.
- **Phase 2 — Advancing the Math**: directional weighting via a `DirectionalMask` trait, Richardson
  Number thermal-physics weighting for SAR reconstruction, and complex-phase extraction from curvelet
  coefficients.
- **Phase 3 — Hardware & Safety**: XLA-compilable FDCT inner-loop module for TPU execution, and an
  encrypted/private `internal_weights` module that hides wreck-signature detection parameters from
  the public crate surface.

The system processes GeoTIFFs, aeromagnetic survey grids, and satellite imagery to locate shipwrecks,
missing persons, downed aircraft, and environmental leaks (e.g., Line 5 pipeline). Precision and
coordinate fidelity are safety-critical: a rounding error or dropped CRS can map a detected anomaly
to the wrong GPS coordinate, sending a SAR team to the wrong location.

---

## Glossary

- **Curvelet_Engine**: The `nauticuvs` crate's core transform module, implementing FDCT forward and
  inverse passes.
- **FDCT**: Fast Discrete Curvelet Transform — the primary signal decomposition algorithm.
- **Subband**: A single directional frequency band produced by the FDCT, stored as a 2-D array of
  complex coefficients.
- **CoefficientStore**: The data structure that holds all Subbands for one FDCT pass, including
  coarse, detail, and fine scales.
- **GeoTIFF_Loader**: The module responsible for reading GeoTIFF files and extracting both pixel
  data and CRS metadata.
- **CRS**: Coordinate Reference System — the projection and datum metadata embedded in a GeoTIFF
  (e.g., WGS-84 / EPSG:4326, UTM zones).
- **GeoTransform**: The six-parameter affine mapping (origin, pixel size, rotation) that converts
  pixel (row, col) to projected (x, y) coordinates.
- **Precision_Flag**: A compile-time or runtime feature flag that selects f32 or f64 as the
  floating-point scalar type for all FDCT calculations.
- **FFT_Backend**: The FFT library used internally by the FDCT — either `rustfft` (pure Rust) or
  `fftw` (C binding), selected via Cargo feature flags.
- **DirectionalMask**: A trait that supplies per-angle, per-scale weighting factors applied to
  curvelet coefficients during reconstruction.
- **Richardson_Number**: The dimensionless ratio Ri = N² / (∂u/∂z)², where N is the Brunt–Väisälä
  buoyancy frequency and ∂u/∂z is the vertical shear of horizontal velocity. Ri < 0.25 indicates
  turbulent mixing; Ri > 1.0 indicates stable stratification.
- **Phase_Coefficient**: The complex argument (angle) of a curvelet coefficient, distinct from its
  amplitude (modulus).
- **XLA_Module**: A Rust module whose inner loops are written to be compilable to XLA (Accelerated
  Linear Algebra) for execution on a TPU.
- **Internal_Weights**: The private module containing wreck-signature detection parameters that must
  not appear in the public crate API.
- **Worker**: Any of `cesarops-aeromagnetic-worker`, `cesarops-satellite-worker`, or
  `sovereign-cloud` — crates that consume `nauticuvs` as a library dependency. Workers receive
  parameters and execute; they do not write new detection logic.
- **Dipole**: A paired positive/negative magnetic anomaly characteristic of ferrous metal objects
  (ships, pipelines, aircraft).
- **Aeromagnetic_Grid**: A 2-D array of total-field magnetic intensity values measured by an
  airborne magnetometer survey.
- **WGS-84**: World Geodetic System 1984 — the GPS datum used for all output coordinates.

---

## Requirements

### Requirement 1: Selectable Floating-Point Precision (f32 / f64)

**User Story:** As a signal-processing engineer, I want to toggle the FDCT between f32 and f64
precision at compile time, so that high-stakes aeromagnetic passes can use f64 to preserve
low-contrast magnetic anomalies that would otherwise be rounded away.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL expose a `precision` Cargo feature flag with two variants: `f32`
   (default) and `f64`.
2. WHEN the `f64` feature flag is active, THE Curvelet_Engine SHALL perform all FDCT arithmetic —
   including FFT passes, wrapping, tiling, and coefficient accumulation — using 64-bit
   floating-point scalars.
3. WHEN the `f32` feature flag is active, THE Curvelet_Engine SHALL perform all FDCT arithmetic
   using 32-bit floating-point scalars.
4. THE FFT_Backend SHALL respect the active Precision_Flag: WHEN `f64` is active, THE FFT_Backend
   SHALL use a 64-bit FFT plan; WHEN `f32` is active, THE FFT_Backend SHALL use a 32-bit FFT plan.
5. WHEN a Worker requests a curvelet forward pass, THE Curvelet_Engine SHALL return coefficients
   whose scalar type matches the active Precision_Flag.
6. THE Curvelet_Engine SHALL expose the active precision type as a public type alias
   (`nauticuvs::Scalar`) so Workers can construct input arrays without hard-coding a float type.
7. IF the `f64` feature flag and the `f32` feature flag are both active simultaneously, THEN THE
   Curvelet_Engine SHALL emit a compile-time error with a descriptive message.
8. THE Curvelet_Engine SHALL preserve at least 10 significant decimal digits in coefficient values
   WHEN the `f64` flag is active, verified by a round-trip property test (forward then inverse
   transform on a synthetic grid).

---

### Requirement 2: SIMD-Aligned Memory Layout for Subband

**User Story:** As a performance engineer, I want the `Subband` struct to use SIMD-aligned memory
allocation, so that large GeoTIFF ingestion does not incur cache-line misses that degrade sub-pixel
signature detection throughput.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL allocate each `Subband`'s coefficient buffer on a 64-byte aligned
   boundary (one full cache line).
2. WHEN a `Subband` is constructed, THE Curvelet_Engine SHALL guarantee that the first element of
   the coefficient buffer is aligned to a 64-byte boundary, regardless of the active Precision_Flag.
3. THE Curvelet_Engine SHALL expose the `Subband` alignment guarantee as a compile-time assertion
   (`assert_eq!(std::mem::align_of::<Subband>(), 64)` or equivalent) so downstream Workers can
   rely on it.
4. WHEN a Worker passes a `Subband` buffer to a SIMD intrinsic or wgpu compute shader, THE
   Curvelet_Engine SHALL ensure no padding bytes exist between coefficient elements within the
   buffer.
5. THE Curvelet_Engine SHALL not increase peak heap allocation by more than 5% compared to an
   unaligned layout for a 4096 × 4096 input grid, verified by a benchmark test.

---

### Requirement 3: CRS-Preserving GeoTIFF Ingestion

**User Story:** As a SAR coordinator, I want every detected anomaly to map back to an exact GPS
coordinate, so that a "pull" identified in an aeromagnetic or satellite pass can be dispatched to
a field team without manual re-projection.

#### Acceptance Criteria

1. THE GeoTIFF_Loader SHALL parse GeoTIFF files and extract both the pixel raster data and the
   embedded CRS metadata (GeoTransform, projection string, and EPSG code where present).
2. WHEN a GeoTIFF file contains a valid GeoTransform, THE GeoTIFF_Loader SHALL store the six
   GeoTransform coefficients with f64 precision.
3. WHEN a GeoTIFF file does not contain a GeoTransform, THE GeoTIFF_Loader SHALL return a
   descriptive error identifying the missing metadata field.
4. THE GeoTIFF_Loader SHALL expose a `pixel_to_wgs84(row: usize, col: usize) -> (f64, f64)`
   method that converts a pixel coordinate to a WGS-84 (latitude, longitude) pair using the
   stored GeoTransform and projection.
5. WHEN the source CRS is a UTM projection, THE GeoTIFF_Loader SHALL reproject the pixel
   coordinate to WGS-84 without loss of sub-meter accuracy (error ≤ 0.5 m at the equator).
6. THE Curvelet_Engine SHALL accept a `GeoTiffInput` type that bundles the pixel array and the
   parsed CRS metadata, and SHALL propagate the CRS metadata unchanged through the full FDCT
   forward pass.
7. WHEN the FDCT forward pass completes, THE CoefficientStore SHALL carry a reference to the
   originating GeoTransform so that any coefficient's spatial origin can be recovered by the
   calling Worker.
8. THE GeoTIFF_Loader SHALL support both `gdal-sys` (via an optional Cargo feature `gdal-support`)
   and a native pure-Rust GeoTIFF parser (default), so Workers without a GDAL installation can
   still ingest GeoTIFFs.
9. IF a GeoTIFF file is corrupt or unreadable, THEN THE GeoTIFF_Loader SHALL return a typed
   `GeoTiffError` variant rather than panicking.
10. FOR ALL valid GeoTIFF files, parsing the GeoTransform then converting pixel (0, 0) to WGS-84
    then converting back to pixel coordinates SHALL recover the original pixel within ±0.001 pixels
    (round-trip property).

---

### Requirement 4: Directional Weighting via DirectionalMask Trait

**User Story:** As an aeromagnetic analyst, I want to apply non-isotropic weights to curvelet
coefficients by angle and scale, so that I can suppress flight-line striping noise while amplifying
dipole signatures perpendicular to the flight path.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL define a public `DirectionalMask` trait with a method
   `weight(scale: usize, angle_deg: f64) -> Scalar` that returns a multiplicative weight in the
   range [0.0, 1.0].
2. WHEN a `DirectionalMask` implementation is supplied to the FDCT reconstruction pass, THE
   Curvelet_Engine SHALL multiply each curvelet coefficient by the weight returned by
   `DirectionalMask::weight` for that coefficient's scale and angle before accumulation.
3. THE Curvelet_Engine SHALL provide a built-in `IdentityMask` implementation of `DirectionalMask`
   that returns 1.0 for all scale and angle inputs, preserving existing behaviour when no mask is
   supplied.
4. THE Curvelet_Engine SHALL provide a built-in `StripeSuppressor` implementation of
   `DirectionalMask` that accepts a flight-path azimuth in degrees and returns a weight of 0.0 for
   angles within ±15° of the flight-path azimuth and 1.0 for angles perpendicular (±90° ± 15°).
5. WHEN a `DirectionalMask` returns a weight outside [0.0, 1.0], THE Curvelet_Engine SHALL clamp
   the weight to [0.0, 1.0] and log a warning.
6. THE Curvelet_Engine SHALL apply `DirectionalMask` weights only during reconstruction; the
   forward pass SHALL store unweighted coefficients so that different masks can be applied to the
   same CoefficientStore without re-running the forward pass.
7. FOR ALL `DirectionalMask` implementations, applying the mask then its complement (weight w and
   weight 1.0 − w) and summing the two reconstructions SHALL produce a result equal to the
   unmasked reconstruction within the precision tolerance of the active Precision_Flag (invariant
   property).

---

### Requirement 5: Richardson Number Thermal-Physics Weighting

**User Story:** As a cold-water SAR analyst, I want the curvelet reconstruction to penalise
coefficients that do not fit the expected buoyancy-to-shear ratio of a 400 ft cold sink rising
through a stratified water column, so that thermal noise is suppressed and genuine cold-water
intrusion signatures are amplified.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL expose a `RichardsonWeighter` struct that accepts a
   `RichardsonProfile` (a vertical profile of buoyancy frequency N² and horizontal shear ∂u/∂z
   at each depth layer) and computes a per-layer Richardson Number Ri = N² / (∂u/∂z)².
2. WHEN `RichardsonWeighter::weight_for_depth(depth_m: f64) -> Scalar` is called, THE
   Curvelet_Engine SHALL return a weight in [0.0, 1.0] that is inversely proportional to Ri:
   coefficients at depths where Ri < 0.25 (turbulent mixing) SHALL receive weight 1.0, and
   coefficients at depths where Ri > 1.0 (stable stratification) SHALL receive weight 0.0, with
   linear interpolation between 0.25 and 1.0.
3. WHEN a `RichardsonWeighter` is supplied to the FDCT reconstruction pass, THE Curvelet_Engine
   SHALL apply the depth-layer weight to each curvelet coefficient whose spatial origin maps to
   that depth layer.
4. IF a `RichardsonProfile` contains a depth layer where ∂u/∂z equals zero, THEN THE
   Curvelet_Engine SHALL assign Ri = +∞ for that layer (perfectly stable) and apply weight 0.0.
5. THE Curvelet_Engine SHALL accept a `RichardsonProfile` with a minimum of 2 depth layers and a
   maximum of 1024 depth layers.
6. THE `RichardsonWeighter` SHALL be composable with a `DirectionalMask`: WHEN both are supplied,
   THE Curvelet_Engine SHALL multiply the directional weight and the Richardson weight before
   applying the combined weight to each coefficient.

---

### Requirement 6: Complex Phase Extraction from Curvelet Coefficients

**User Story:** As a magnetic anomaly analyst, I want to access the phase angle of each curvelet
coefficient separately from its amplitude, so that I can track phase shifts in the magnetic field
that indicate human-made metal structures more reliably than amplitude spikes alone.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL store each curvelet coefficient as a complex number with explicit
   real and imaginary parts, using the active Precision_Flag scalar type.
2. THE Curvelet_Engine SHALL expose a `phase_map(scale: usize) -> Array2<Scalar>` method on
   `CoefficientStore` that returns the phase angle (in radians, range [−π, π]) of every
   coefficient in the specified scale.
3. THE Curvelet_Engine SHALL expose an `amplitude_map(scale: usize) -> Array2<Scalar>` method on
   `CoefficientStore` that returns the modulus of every coefficient in the specified scale.
4. WHEN `phase_map` and `amplitude_map` are called for the same scale, THE Curvelet_Engine SHALL
   guarantee that the returned arrays have identical shape and that element (i, j) in each array
   corresponds to the same spatial coefficient.
5. THE Curvelet_Engine SHALL expose a `phase_coherence(scale: usize, window_radius: usize) ->
   Array2<Scalar>` method that computes the local phase coherence (mean resultant length of phase
   vectors in a sliding window) for the specified scale.
6. IF `phase_map` is called with a scale index that does not exist in the `CoefficientStore`, THEN
   THE Curvelet_Engine SHALL return a typed `ScaleIndexError` rather than panicking.
7. FOR ALL valid input grids, the phase of a coefficient computed by the forward pass SHALL equal
   the phase recovered by computing `atan2(imag, real)` on the stored complex coefficient
   (round-trip property, tolerance ≤ 1e-6 radians for f64, ≤ 1e-4 radians for f32).

---

### Requirement 7: XLA-Compilable FDCT Inner-Loop Module

**User Story:** As a hardware engineer, I want the FDCT wrapping and tiling inner loops refactored
into a separate module that can be compiled to XLA for TPU execution, so that large aeromagnetic
survey grids can be processed at TPU throughput without rewriting the algorithm.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL isolate the FDCT wrapping step and the FDCT tiling step into a
   dedicated `fdct_kernels` module with no dependencies on Rust standard-library heap allocation
   within the hot path.
2. THE `fdct_kernels` module SHALL expose each kernel as a pure function with inputs and outputs
   expressed as flat, contiguous slices of the active Precision_Flag scalar type, with no
   interior mutability or thread-local state.
3. WHEN the `xla` Cargo feature flag is active, THE `fdct_kernels` module SHALL compile without
   errors using the `xla` crate's `#[xla::kernel]` attribute on each kernel function.
4. WHEN the `xla` feature flag is inactive, THE `fdct_kernels` module SHALL compile and execute
   identically on CPU using standard Rust, with no XLA-specific code paths active.
5. THE `fdct_kernels` module SHALL produce numerically identical results (within the active
   Precision_Flag tolerance) whether executed on CPU or via XLA, verified by a property test that
   runs both paths on the same input and compares outputs element-wise.
6. THE `fdct_kernels` module SHALL not expose any wreck-signature detection parameters or
   Internal_Weights constants.

---

### Requirement 8: Private Internal Weights Module

**User Story:** As the system owner, I want wreck-signature detection logic isolated in a private
`internal_weights` module that is not part of the public `nauticuvs` crate API, so that the
generic curvelet tool can be published openly without exposing the exact detection parameters that
took years to calibrate.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL place all wreck-signature thresholds, trained weight vectors, and
   detection heuristics in a module named `internal_weights` that is declared `pub(crate)` or
   lower visibility.
2. THE `internal_weights` module SHALL not re-export any symbol through the crate's public API
   (`pub use`, `pub mod`, or `pub fn` at crate root).
3. WHEN the `nauticuvs` crate is compiled as a library dependency by an external crate, THE
   Curvelet_Engine SHALL ensure that no `internal_weights` symbol appears in the generated
   documentation (`cargo doc`) or the public type system.
4. THE Curvelet_Engine SHALL provide a public `DetectionConfig` struct that accepts opaque
   detection parameters as serialised bytes (e.g., a `&[u8]` blob), so Workers can supply
   parameters at runtime without the parameter schema being visible in the public API.
5. WHEN a Worker calls `curvelet_forward` or any public reconstruction function, THE
   Curvelet_Engine SHALL not require the Worker to import or reference any symbol from
   `internal_weights`.
6. THE `internal_weights` module SHALL be covered by at least one integration test that verifies
   the detection parameters produce the expected output on a known synthetic dipole grid, without
   exposing the parameter values in the test output.

---

### Requirement 9: Backward Compatibility and Worker Integration

**User Story:** As a worker developer, I want the precision engine upgrades to be additive and
opt-in, so that existing workers continue to compile and produce correct results without code
changes.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL preserve the existing `curvelet_forward(grid: &Array2<f32>, scales:
   usize) -> Result<CoefficientStore, CurveletError>` public signature as the default entry point
   WHEN the `f32` feature flag is active (or no precision flag is specified).
2. WHEN a Worker that was compiled against the previous `nauticuvs` API is recompiled against the
   precision engine, THE Curvelet_Engine SHALL produce no new compile errors in the Worker's
   existing call sites.
3. THE Curvelet_Engine SHALL expose all new capabilities (GeoTIFF ingestion, DirectionalMask,
   RichardsonWeighter, phase extraction, XLA kernels) as additive public items that do not alter
   the signatures of existing public functions.
4. WHEN the `cesarops-aeromagnetic-worker` calls `curvelet_forward` with a 64×64 f32 window and
   4 scales, THE Curvelet_Engine SHALL return a `CoefficientStore` whose `detail` and `fine`
   fields are accessible with the same field names as before.
5. THE Curvelet_Engine SHALL compile without warnings under `cargo build --workspace` with the
   default feature set.

---

### Requirement 10: Parser and Serialisation Round-Trip Integrity

**User Story:** As a data integrity engineer, I want all data structures that cross process
boundaries (CoefficientStore, GeoTransform, RichardsonProfile) to serialise and deserialise
without loss, so that results stored to disk or transmitted between workers are bit-for-bit
reproducible.

#### Acceptance Criteria

1. THE Curvelet_Engine SHALL implement `serde::Serialize` and `serde::Deserialize` for
   `CoefficientStore`, `GeoTransform`, and `RichardsonProfile`.
2. WHEN a `CoefficientStore` is serialised to JSON and then deserialised, THE Curvelet_Engine
   SHALL produce a `CoefficientStore` whose coefficient values are equal to the original within
   the active Precision_Flag tolerance.
3. WHEN a `GeoTransform` is serialised to JSON and then deserialised, THE Curvelet_Engine SHALL
   produce a `GeoTransform` whose six coefficients are equal to the original to full f64
   precision.
4. FOR ALL valid `CoefficientStore` values, serialising then deserialising SHALL produce an
   equivalent value (round-trip property): `deserialise(serialise(x)) == x`.
5. FOR ALL valid `GeoTransform` values, serialising then deserialising SHALL produce an equivalent
   value (round-trip property): `deserialise(serialise(x)) == x`.
6. THE Curvelet_Engine SHALL expose a `CoefficientStore::checksum() -> u64` method that returns a
   deterministic hash of all coefficient values, so Workers can verify data integrity after
   transmission without deserialising the full store.
