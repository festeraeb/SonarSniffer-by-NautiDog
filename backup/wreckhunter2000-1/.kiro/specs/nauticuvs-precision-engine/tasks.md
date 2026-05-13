# Implementation Plan: nauticuvs Precision Engine

## Overview

Implement the `nauticuvs` precision engine as a strictly additive upgrade to the `nauticuvs` Rust
crate. All tasks build incrementally from the dependency foundation upward, ending with the public
API surface and test suite. The existing `curvelet_forward(&window, 4)` call site in
`cesarops-aeromagnetic-worker` must compile and produce correct results at every checkpoint.

## Tasks

- [x] 1. Update `nauticuvs/Cargo.toml` with new dependencies and feature flags
  - Add `num-complex = "0.4.6"`, `rustfft = "6.2.0"`, `aligned-vec = "0.5.0"`,
    `thiserror = "1.0.69"`, `tiff = "0.9.1"`, `proj = "0.27.0"` to `[dependencies]`
  - Add optional deps: `fftw = { version = "0.8.0", optional = true }`,
    `gdal = { version = "0.16.0", optional = true }`,
    `xla = { version = "0.0.9", optional = true }`
  - Add `[dev-dependencies]`: `proptest = "1.5.0"`, `approx = "0.5.1"`,
    `criterion = { version = "0.5.1", features = ["html_reports"] }`
  - Add `[features]` section: `default = []`, `f64 = []`, `f32-explicit = []`,
    `fftw = ["dep:fftw"]`, `gdal-support = ["dep:gdal"]`, `xla = ["dep:xla"]`
  - Bump crate version to `"0.2.0"`
  - _Requirements: 1.1, 1.4, 7.3, 7.4, 3.8_

- [x] 2. Create `src/precision.rs` — Scalar type alias and mutual-exclusion guard
  - Define `pub type Scalar = f64` under `#[cfg(feature = "f64")]`
  - Define `pub type Scalar = f32` under `#[cfg(not(feature = "f64"))]`
  - Add `#[cfg(all(feature = "f32-explicit", feature = "f64"))] compile_error!(...)` with a
    descriptive message
  - Re-export `pub use num_complex::Complex` and define `pub type C = Complex<Scalar>`
  - _Requirements: 1.1, 1.2, 1.3, 1.6, 1.7_

- [x] 3. Create error types in their respective modules
  - [x] 3.1 Create `src/geo/mod.rs` (empty re-export stub) and `src/geo/transform.rs` with
    `GeoTiffError` enum using `#[derive(Debug, thiserror::Error)]` — variants:
    `MissingGeoTransform`, `CorruptData(String)`, `UnsupportedFormat(String)`,
    `ReprojectError(String)`, `Io(#[from] std::io::Error)`
    - _Requirements: 3.3, 3.9_
  - [x] 3.2 Create `src/weights/mod.rs` (empty re-export stub) and
    `src/weights/richardson.rs` with `RichardsonError` enum — variants:
    `TooFewLayers(usize)`, `TooManyLayers(usize)`, `MismatchedLengths`
    - _Requirements: 5.5_
  - [x] 3.3 Create `src/curvelet/mod.rs` (empty stub) and
    `src/curvelet/coefficient_store.rs` with `CurveletError` and `ScaleIndexError` using
    `thiserror` — `CurveletError` variants: `ZeroDimension`, `InvalidScaleCount`,
    `FftError(String)`, `GeoTiff(#[from] GeoTiffError)`
    - _Requirements: 3.9, 6.6_

- [x] 4. Create `src/curvelet/coefficient_store.rs` — `Subband` and `CoefficientStore`
  - Define `#[repr(C, align(64))] pub struct Subband` with `data: AlignedVec<Complex<Scalar>>`,
    `rows`, `cols`, `scale: usize`, `angle_deg: f64` fields
  - Add compile-time assertion `const _: () = assert!(std::mem::align_of::<Subband>() >= 64)`
  - Define `pub struct CoefficientStore` with fields `coarse: Subband`,
    `detail: Vec<Vec<Subband>>`, `fine: Vec<Complex<Scalar>>`,
    `geo_transform: Option<GeoTransform>`, `num_scales: usize` — matching the aeromagnetic
    worker's `coeffs.detail` and `coeffs.fine` access patterns exactly
  - Add stub `impl CoefficientStore` with `phase_map`, `amplitude_map`, `phase_coherence`,
    `checksum` method signatures returning `todo!()` (implementations come in task 12)
  - _Requirements: 2.1, 2.2, 2.3, 2.4, 6.1, 9.4_

- [x] 5. Create `src/fdct_kernels/` — pure slice kernel functions
  - Create `src/fdct_kernels/mod.rs` re-exporting all three sub-modules
  - Create `src/fdct_kernels/window.rs` with `WedgeParams` struct (Copy, Clone, no heap
    pointers) and `pub fn apply_wedge_window(fft_plane, plane_rows, plane_cols, dst,
    wedge_params)` annotated with `#[cfg_attr(feature = "xla", xla::kernel)]`
  - Create `src/fdct_kernels/wrap.rs` with `pub fn wrap_to_canonical(src, src_rows, src_cols,
    dst, dst_rows, dst_cols, wrap_offsets)` — no heap allocation in hot path, annotated with
    `#[cfg_attr(feature = "xla", xla::kernel)]`
  - Create `src/fdct_kernels/tile.rs` with `pub fn ifft_tile(src, rows, cols, dst,
    fft_scratch)` — caller-provided scratch buffer, annotated with
    `#[cfg_attr(feature = "xla", xla::kernel)]`
  - _Requirements: 7.1, 7.2, 7.3, 7.4_

- [x] 6. Create `src/fft_backend.rs` — FFT backend abstraction
  - Define `pub(crate) trait FftBackend` with `fft2d` and `ifft2d` methods operating on
    `&[Complex<Scalar>]` slices
  - Implement `pub(crate) struct RustFftBackend` using `rustfft` — `f32` plan when `Scalar=f32`,
    `f64` plan when `Scalar=f64` (via `FftNum` trait bound)
  - Add `#[cfg(feature = "fftw")] pub(crate) struct FftwBackend` stub
  - Define `type ActiveBackend` alias: `FftwBackend` when `fftw` feature active, else
    `RustFftBackend`
  - _Requirements: 1.4, 7.1_

- [x] 7. Implement `src/curvelet/mod.rs` — forward and inverse curvelet passes
  - Implement `pub fn curvelet_forward(grid: &Array2<Scalar>, scales: usize) ->
    Result<CoefficientStore, CurveletError>` — the backward-compatible entry point:
    2-D FFT via `ActiveBackend`, wedge windowing via `fdct_kernels::window`, wrapping via
    `fdct_kernels::wrap`, IFFT tiling via `fdct_kernels::tile`, populate `detail`, `fine`,
    `coarse` fields
  - Implement `pub fn curvelet_forward_geo(input: &GeoTiffInput, scales: usize) ->
    Result<CoefficientStore, CurveletError>` — delegates to `curvelet_forward` and attaches
    `geo_transform` to the returned `CoefficientStore`
  - Implement `pub fn curvelet_inverse(store: &CoefficientStore, mask: Option<&dyn
    DirectionalMask>, weighter: Option<&RichardsonWeighter>) ->
    Result<Array2<Scalar>, CurveletError>` — adjoint FDCT with optional mask and Richardson
    weighting; combined weight = `clamp(mask.weight(s,a), 0,1) * weighter.weight_for_depth(d)`
  - _Requirements: 1.2, 1.3, 1.5, 4.2, 4.6, 5.3, 5.6, 9.1, 9.4_

- [x] 8. Create `src/geo/transform.rs` — `GeoTransform` and coordinate conversion
  - Define `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)] pub struct GeoTransform`
    with `coeffs: [f64; 6]`, `epsg: Option<u32>`, `projection_wkt: Option<String>`
  - Implement `pub fn pixel_to_projected(&self, row: usize, col: usize) -> (f64, f64)` using
    GDAL-convention affine formula: `x = gt[0] + col*gt[1] + row*gt[2]`,
    `y = gt[3] + col*gt[4] + row*gt[5]`
  - Implement `pub fn pixel_to_wgs84(&self, row: usize, col: usize) ->
    Result<(f64, f64), GeoTiffError>` using the `proj` crate to reproject from source CRS to
    WGS-84 (EPSG:4326); sub-meter accuracy at equator
  - Implement `pub fn wgs84_to_pixel(&self, lat: f64, lon: f64) ->
    Result<(f64, f64), GeoTiffError>` — inverse reprojection for round-trip verification
  - _Requirements: 3.2, 3.4, 3.5, 3.10_

- [x] 9. Create `src/geo/geotiff.rs` — `GeoTiffInput` and pure-Rust TIFF tag parsing
  - Define `pub struct GeoTiffInput { pub pixels: Array2<Scalar>, pub geo_transform:
    GeoTransform }`
  - Implement `pub fn from_bytes(data: &[u8]) -> Result<Self, GeoTiffError>` — use `tiff` crate
    to read IFD tags 33550 (ModelPixelScaleTag), 33922 (ModelTiepointTag), 34264
    (ModelTransformationTag), 34736/34737 (GeoDoubleParamsTag/GeoAsciiParamsTag); derive the six
    GDAL-convention GeoTransform coefficients; return `GeoTiffError::MissingGeoTransform` if
    tags absent, `GeoTiffError::CorruptData` if TIFF is malformed
  - Implement `pub fn from_path(path: &Path) -> Result<Self, GeoTiffError>` — reads file to
    bytes and delegates to `from_bytes`
  - Add `#[cfg(feature = "gdal-support")]` branch in `from_bytes` that uses the `gdal` crate
    instead of manual tag parsing
  - _Requirements: 3.1, 3.2, 3.3, 3.8, 3.9_

- [x] 10. Create `src/weights/directional.rs` — `DirectionalMask` trait and built-in impls
  - Define `pub trait DirectionalMask: Send + Sync` with
    `fn weight(&self, scale: usize, angle_deg: f64) -> Scalar`
  - Implement `pub struct IdentityMask` returning `1.0` for all inputs
  - Implement `pub struct StripeSuppressor { pub flight_azimuth_deg: f64 }` — returns `0.0`
    within ±15° of flight azimuth, `1.0` within ±15° of perpendicular (azimuth ± 90°), linear
    interpolation in transition zones
  - Implement `pub struct ComplementMask<'a> { inner: &'a dyn DirectionalMask }` returning
    `1.0 - inner.weight(scale, angle_deg)` — used for the mask + complement invariant property
  - _Requirements: 4.1, 4.3, 4.4, 4.7_

- [x] 11. Create `src/weights/richardson.rs` — `RichardsonProfile` and `RichardsonWeighter`
  - Define `#[derive(Debug, Clone, Serialize, Deserialize)] pub struct RichardsonProfile` with
    `depths_m: Vec<f64>`, `n_squared: Vec<f64>`, `shear: Vec<f64>`
  - Implement `pub fn RichardsonProfile::new(...)` — validate layer count (2–1024) and equal
    lengths; return `RichardsonError` variants on failure
  - Implement `pub struct RichardsonWeighter { profile, ri: Vec<f64> }` — pre-compute
    `ri[i] = n_squared[i] / shear[i].powi(2)`, using `f64::INFINITY` where `shear[i] == 0.0`
  - Implement `pub fn weight_for_depth(&self, depth_m: f64) -> Scalar` — interpolate between
    nearest depth layers; map Ri < 0.25 → 1.0, Ri > 1.0 → 0.0, linear between 0.25 and 1.0
  - _Requirements: 5.1, 5.2, 5.4, 5.5_

- [x] 12. Implement `src/curvelet/phase.rs` — phase and amplitude extraction
  - Implement `CoefficientStore::phase_map(scale) -> Result<Array2<Scalar>, ScaleIndexError>` —
    return `atan2(c.im, c.re)` for each coefficient in `detail[scale]`; validate scale index
    and return `ScaleIndexError` if out of bounds
  - Implement `CoefficientStore::amplitude_map(scale) -> Result<Array2<Scalar>, ScaleIndexError>`
    — return `c.norm()` for each coefficient; same shape guarantee as `phase_map`
  - Implement `CoefficientStore::phase_coherence(scale, window_radius) ->
    Result<Array2<Scalar>, ScaleIndexError>` — compute mean resultant length of phase vectors
    in a sliding window of radius `window_radius`; result in [0.0, 1.0]
  - Implement `CoefficientStore::checksum() -> u64` — deterministic FNV-1a hash over all
    coefficient bytes in `coarse`, `detail`, and `fine` in a fixed traversal order
  - _Requirements: 6.1, 6.2, 6.3, 6.4, 6.5, 6.6, 10.6_

- [x] 13. Create `src/internal_weights/mod.rs` and `src/detection.rs`
  - Create `src/internal_weights/mod.rs` declared as `mod internal_weights` (no `pub`) in
    `lib.rs`; define `pub(crate) struct WreckSignatureParams` with
    `dipole_energy_threshold: Scalar`, `phase_coherence_min: Scalar`,
    `scale_weights: [Scalar; 8]`; implement `pub(crate) fn load_params(blob: &[u8]) ->
    Result<WreckSignatureParams, ParamError>` with a `ParamError` type
  - Create `src/detection.rs` with `pub struct DetectionConfig(Vec<u8>)` and
    `pub fn from_bytes(blob: &[u8]) -> Self`; add a `pub fn detect_anomaly` function that
    deserialises the blob via `internal_weights::load_params` internally — no
    `internal_weights` symbol appears in the public API
  - _Requirements: 8.1, 8.2, 8.3, 8.4, 8.5_

- [x] 14. Add `serde` impls for `CoefficientStore`, `GeoTransform`, and `RichardsonProfile`
  - Add `#[derive(Serialize, Deserialize)]` to `CoefficientStore` — handle `AlignedVec` by
    serialising `Subband::data` as a `Vec<[Scalar; 2]>` (real, imag pairs) and reconstructing
    the aligned buffer on deserialise
  - Verify `GeoTransform` already derives `Serialize, Deserialize` (added in task 8)
  - Verify `RichardsonProfile` already derives `Serialize, Deserialize` (added in task 11)
  - Add a round-trip smoke test inline in `coefficient_store.rs`:
    `serde_json::from_str(&serde_json::to_string(&store).unwrap()).unwrap()`
  - _Requirements: 10.1, 10.2, 10.3, 10.4, 10.5_

- [ ] 15. Update `src/lib.rs` — re-exports and public API surface
  - Declare all new modules: `pub mod curvelet`, `pub mod fdct_kernels`, `pub mod geo`,
    `pub mod weights`, `mod fft_backend`, `mod internal_weights`, `mod precision`,
    `mod detection`
  - Add `pub use precision::Scalar`
  - Add `pub use curvelet::curvelet_forward` (backward-compatible entry point)
  - Add `pub use curvelet::mod::{curvelet_forward_geo, curvelet_inverse}`
  - Add `pub use curvelet::coefficient_store::{CoefficientStore, Subband, CurveletError,
    ScaleIndexError}`
  - Add `pub use geo::transform::GeoTransform`, `pub use geo::geotiff::{GeoTiffInput,
    GeoTiffError}`
  - Add `pub use weights::directional::{DirectionalMask, IdentityMask, StripeSuppressor,
    ComplementMask}`
  - Add `pub use weights::richardson::{RichardsonWeighter, RichardsonProfile}`
  - Add `pub use detection::DetectionConfig`
  - Ensure `internal_weights` has no `pub` qualifier — crate-private only
  - _Requirements: 8.2, 8.3, 9.1, 9.2, 9.3_

- [x] 16. Checkpoint — verify backward compatibility and clean build
  - Ensure all tests pass, ask the user if questions arise.
  - Confirm `cesarops-aeromagnetic-worker` still compiles: `curvelet_forward(&window, 4)`,
    `coeffs.detail`, `coeffs.fine`, and `.norm_sqr()` on coefficients all resolve without error
  - Run `cargo build --workspace` with default features and confirm zero warnings
  - _Requirements: 9.1, 9.2, 9.4, 9.5_

- [ ] 17. Write property-based tests — `tests/prop_*.rs`
  - [ ] 17.1 Create `tests/prop_precision.rs`
    - Define `arb_grid` generator for `Array2<Scalar>` with finite, non-NaN values
    - **Property 1: FDCT round-trip precision (f64)** — forward then inverse, relative error
      < 1e-10 per element when `f64` feature active
    - **Validates: Requirements 1.8**
    - [ ]* 17.1a Write proptest for Property 1
    - **Property 2: Coefficient scalar type matches active Precision_Flag** — element size of
      coefficients equals `size_of::<Scalar>()`
    - **Validates: Requirements 1.5**
    - [ ]* 17.1b Write proptest for Property 2

  - [ ] 17.2 Create `tests/prop_subband_alignment.rs`
    - **Property 3: Subband buffer is 64-byte aligned** — `ptr as usize % 64 == 0` for every
      `Subband` produced by `curvelet_forward`
    - **Validates: Requirements 2.1, 2.2**
    - [ ]* 17.2a Write proptest for Property 3
    - **Property 4: Subband buffer has no inter-element padding** — stride equals
      `size_of::<Complex<Scalar>>()`
    - **Validates: Requirements 2.4**
    - [ ]* 17.2b Write proptest for Property 4

  - [ ] 17.3 Create `tests/prop_geo_roundtrip.rs`
    - Define `arb_geo_transform` generator (finite, non-degenerate coefficients)
    - **Property 5: GeoTransform round-trip (pixel → WGS-84 → pixel)** — pixel (0,0) round-trips
      within ±0.001 pixels
    - **Validates: Requirements 3.10**
    - [ ]* 17.3a Write proptest for Property 5
    - **Property 6: CRS metadata propagated unchanged through FDCT** — `geo_transform` coeffs
      in `CoefficientStore` are bit-for-bit identical to input
    - **Validates: Requirements 3.6, 3.7**
    - [ ]* 17.3b Write proptest for Property 6

  - [ ] 17.4 Create `tests/prop_directional_mask.rs`
    - **Property 7: DirectionalMask weights are clamped to [0.0, 1.0]** — engine clamps
      out-of-range weights before applying to coefficients
    - **Validates: Requirements 4.5**
    - [ ]* 17.4a Write proptest for Property 7
    - **Property 8: Mask + complement = identity reconstruction** — sum of masked and
      complement-masked reconstructions equals unmasked reconstruction within precision tolerance
    - **Validates: Requirements 4.7**
    - [ ]* 17.4b Write proptest for Property 8
    - **Property 9: IdentityMask returns 1.0 for all inputs**
    - **Validates: Requirements 4.3**
    - [ ]* 17.4c Write proptest for Property 9

  - [ ] 17.5 Create `tests/prop_richardson.rs`
    - Define `arb_richardson_profile` generator (2–1024 layers, finite values)
    - **Property 10: Richardson weight is in [0.0, 1.0] and follows piecewise linear rule**
    - **Validates: Requirements 5.2**
    - [ ]* 17.5a Write proptest for Property 10
    - **Property 11: Richardson weight composition with DirectionalMask** — combined weight
      equals `clamp(mask.weight(s,a), 0,1) * weighter.weight_for_depth(d)`
    - **Validates: Requirements 5.6**
    - [ ]* 17.5b Write proptest for Property 11
    - **Property 12: RichardsonProfile layer count bounds** — < 2 or > 1024 layers returns
      error; 2–1024 layers succeeds
    - **Validates: Requirements 5.5**
    - [ ]* 17.5c Write proptest for Property 12

  - [ ] 17.6 Create `tests/prop_phase.rs`
    - Define `arb_coefficient_store` generator (run `curvelet_forward` on `arb_grid`)
    - **Property 13: phase_map values are in [−π, π]**
    - **Validates: Requirements 6.2**
    - [ ]* 17.6a Write proptest for Property 13
    - **Property 14: phase_map equals atan2(imag, real)** — tolerance ≤ 1e-6 rad (f64) or
      ≤ 1e-4 rad (f32)
    - **Validates: Requirements 6.7**
    - [ ]* 17.6b Write proptest for Property 14
    - **Property 15: phase_map and amplitude_map have identical shape**
    - **Validates: Requirements 6.4**
    - [ ]* 17.6c Write proptest for Property 15
    - **Property 16: phase_coherence values are in [0.0, 1.0]**
    - **Validates: Requirements 6.5**
    - [ ]* 17.6d Write proptest for Property 16

  - [ ] 17.7 Create `tests/prop_serde.rs`
    - **Property 17: CoefficientStore serde round-trip** — JSON serialise then deserialise
      produces equal coefficient values within precision tolerance
    - **Validates: Requirements 10.4**
    - [ ]* 17.7a Write proptest for Property 17
    - **Property 18: GeoTransform serde round-trip** — six coefficients are bit-for-bit
      identical after JSON round-trip
    - **Validates: Requirements 10.5**
    - [ ]* 17.7b Write proptest for Property 18
    - **Property 19: checksum is deterministic** — `checksum()` returns the same `u64` on two
      calls to the same value; equal stores produce equal checksums
    - **Validates: Requirements 10.6**
    - [ ]* 17.7c Write proptest for Property 19

- [ ] 18. Write integration tests
  - [x] 18.1 Create `tests/integration_backward_compat.rs` — backward compatibility smoke test
    - Reproduce the exact aeromagnetic worker call pattern:
      `let window = Array2::<f32>::zeros((64, 64)); let coeffs = curvelet_forward(&window, 4).unwrap();`
    - Assert `coeffs.detail[0][0].iter().map(|c| c.norm_sqr()).sum::<f64>()` compiles and runs
    - Assert `coeffs.fine.iter().map(|c| c.norm_sqr()).sum::<f64>()` compiles and runs
    - This test fails to compile if `detail` or `fine` field names change
    - _Requirements: 9.1, 9.2, 9.4_
  - [ ]* 18.2 Create `tests/integration_internal_weights.rs` — internal weights dipole test
    - Create `tests/test_fixtures/test_params.bin` with a minimal synthetic parameter blob
    - Implement a `synthetic_dipole_grid(128, 128)` helper that injects a dipole anomaly
    - Call `DetectionConfig::from_bytes(include_bytes!("test_fixtures/test_params.bin"))` and
      `detect_anomaly(&grid, &config)` — assert `result.score > 0.5` without printing parameter
      values
    - Verify no `internal_weights` symbol is accessible from the test (it is `pub(crate)`)
    - _Requirements: 8.6_

- [x] 19. Final checkpoint — clean build and full test suite
  - Ensure all tests pass, ask the user if questions arise.
  - Run `cargo build --workspace` with default features — confirm zero warnings
  - Confirm `cargo doc --no-deps` produces no `internal_weights` symbols in the public docs
  - _Requirements: 8.3, 9.5_

## Notes

- Tasks marked with `*` are optional and can be skipped for faster MVP
- Each task references specific requirements for traceability
- Checkpoints (tasks 16 and 19) ensure incremental validation
- Property tests (task 17) validate the 19 universal correctness properties from the design
- Unit tests validate specific examples and error conditions
- The `detail` and `fine` field names on `CoefficientStore` are frozen — changing them breaks
  the aeromagnetic worker without a compile error in `nauticuvs` itself
- The `xla` feature uses `#[cfg_attr(feature = "xla", xla::kernel)]` — a no-op on CPU paths
- All public functions return `Result<_, E>`; no `unwrap()` or `expect()` on user-supplied data
