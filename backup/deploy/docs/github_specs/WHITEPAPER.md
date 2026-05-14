# SonarSniffer Rust — Technical White Paper & Marching Orders
**Version:** 1.0 · March 2026  
**Author:** Derived from full codebase audit  
**Purpose:** Exact-state documentation, gap analysis vs. the prosumer vision, and ordered implementation plan for AI/human coders

---

## Part I — What the Code Actually Does Today

### 1.1 System Architecture

```
.rsd File
   │
   ▼
GarminRSDParser (garmin_rsd_parser.rs)
   ├─ Magic-byte sync (self-healing resync on corrupt records)
   ├─ Generation detection → Gen1Classic / UHD / UHD2 / Unknown
   ├─ Per-record varstruct parse (header + body + sonar blob)
   ├─ SampleHint: U8 (Gen1) vs I16 (UHD/UHD2)
   ├─ Self-healing sample_count from sonar_size when field7 unreliable
   ├─ normalize_nadir_direction() → auto-flip reversed channels
   └─ ParseResult { pings: Vec<Ping>, channels, healing_actions, … }
          │
          ▼
   ChannelDiscovery (channel_discovery.rs)
   ├─ Signal archetype classification per channel (SideVü, DownVü/ClearVü, DepthTemp, Noise)
   ├─ Frequency tier via sample entropy: Detail (UHD/CHIRP high-entropy) vs Context
   ├─ Nadir-gap width measurement per channel
   ├─ Port/Starboard pairing: match nadir-gap widths + COG heading assignment
   └─ DiscoveryResult { profiles, primary_sidescan_pair, best_center_channel }
          │
     ┌────┴────────────────────────────────────────────────────────────┐
     │ Path A: PNG Waterfall + Stitched Mosaic Image (outputs.rs)       │
     │   render_sidescan_stitched()                                      │
     │   ├─ Per-ping pixel row: sample → colormap → RgbImage            │
     │   ├─ TVG (video_enhanced/tvg.rs) applied to each ping            │
     │   ├─ Nadir mode: Stitch/Fill/Raw                                  │
     │   ├─ blend_nadir_seam() — Gaussian 28px blur at center seam      │
     │   └─ PNG output + optional GIF/MP4 video                         │
     │                                                                    │
     │ Path B: Geo-referenced Grid Mosaic (mosaic/engine.rs)            │
     │   build_mosaic()                                                   │
     │   ├─ GPS → Web Mercator meters                                    │
     │   ├─ TVG LUT precomputed (alpha/beta exponential)                 │
     │   ├─ Slant-range correction: slant = √(ground² + depth²)         │
     │   ├─ Trapezoidal interpolation between consecutive ping pairs     │
     │   ├─ Gaussian alpha feathering (sweet spot 60% swath)            │
     │   ├─ Per-channel p2/p98 histogram normalization                  │
     │   ├─ Edge fading at near/far range extremes                      │
     │   └─ MosaicGrid: f32 accumulator + weight array                  │
     │          │                                                         │
     │          ▼                                                         │
     │   mosaic/blending.rs                                               │
     │   ├─ export_mbtiles() → PNG tiles in SQLite (Deflated zip)       │
     │   ├─ export_kmz() → KML + PNG overlay (Deflated zip)             │
     │   └─ mosaic/projection.rs → older grid path (redundant)          │
     └────────────────────────────────────────────────────────────────┘
          │
          ▼
   Tauri GUI (src/index.html + src/main.js)
   ├─ Single-file process panel, batch panel, deep debug panel
   ├─ Output options: PNG, video, KML, KMZ, MBTiles, mosaic
   ├─ Nadir radio: Stitch / Fill (downscan) / Raw
   └─ Colormap selector, remove water column checkbox
```

---

### 1.2 The Parser — Exact Flow (`garmin_rsd_parser.rs`)

**Step 1: File read + magic-byte sync**  
Reads full file into memory. Scans for the record header magic (`MAGIC_REC_HDR`) using a multi-candidate list loaded from a `.magic` sidecar or built in from known firmware constants. On first-sync failure it returns an error; on mid-file corrupt records it skips forward to the next magic (self-healing).

**Step 2: Generation detection**  
`detect_generation()` reads the first N records and determines hardware generation:
- `Gen1Classic` — 8-bit samples, channel IDs 0–3, field7 is XID/TVG metadata  
- `UHD` — 16-bit signed samples, channels 4–7  
- `UHD2` — 16-bit signed, channels 8–21 (includes dual-freq ClearVü)  
- `Unknown` — triggers heuristic path  

**Step 3: Per-record decode (`try_parse_record`)**  
Each record has two varstructs (header + body) followed by a raw sonar blob.  
- Header: magic, sequence, data_size, timestamp  
- Body: channel_id (f0), depth (f1), lat (f9), lon (f10), temp (f14), heading (f16), pitch (f17), roll (f18)  
- Sonar blob: decoded as u8 → Vec<u16> (Gen1) or i16 → Vec<u16> (UHD/UHD2)  
- Self-healing: `infer_sample_hint_from_layout()` overrides static mapping when sonar_size evidence contradicts header  
- `normalize_sample_count()` reconciles field7 vs sonar_size with ±8 byte tolerance  

**Step 4: Post-parse healing**  
- `normalize_nadir_direction()` — scans the first 100 pings per channel, measures the nadir gap at left vs right edge using a P15/P90 threshold. If nadir consistently appears at the right end (wrong direction), reverses all samples for that channel. This is the "channel flip" detection.  
- `compute_file_fingerprint()` + `record_discovery()` — logs unknown generation patterns to a local JSONL healing cache for future lookup.  

**Step 5: Static channel table fallback (`map_channel_info`)**  
A hardcoded table mapping channel IDs to `(beam_type, generation)`:
```
0–3   Gen1Classic  (port/star/down/depth)
4–7   UHD          (port/star/down/depth)
8–11  UHD2 8-series
12–15 UHD2 10-series/CV100
16–21 UHD2 ClearVü dual-freq
```
**This path is now a fallback only.** Channel role assignment is primarily done by `ChannelDiscovery`.

---

### 1.3 Channel Discovery — Data-Driven Classification (`channel_discovery.rs`)

This module is the crown jewel of the self-healing architecture. It replaces ALL static channel lookups with signal analysis:

**Signal Archetype Classification:**
- `SideVü` — long sample array, nadir gap (near-zero samples) in a consistent position near one edge, then rising returns  
- `DownVü/ClearVü` — strong early return (first bottom hit), rapid amplitude decay, nadir gap either absent or narrow  
- `DepthTemp` — very few non-zero samples, no spatial structure  
- `Noise` — random or all-zero

**Frequency Tier:**  
Computes sample-level Shannon entropy. High entropy → Detail (UHD/CHIRP active sub-bottom or high-frequency sidescan). Low entropy → Context (standard 455kHz sidescan). This is the closest existing analog to the "signal variance" approach requested in the plan.

**Port/Starboard Assignment:**  
Measures nadir-gap width for each SideVü channel across the first 100 pings. Matches the pair with most-similar gap widths (they are the same transducer depth, so widths should match). Uses vessel COG (heading diff between consecutive pings) to determine which arm is geometrically port vs. starboard.

**Nadir-flip correction:**  
Independently reproduced by both `normalize_nadir_direction()` (parser) and the discovery module — belt-and-suspenders approach.

---

### 1.4 TVG — Time-Varied Gain (`video_enhanced/tvg.rs`)

Fully implemented in **Float32**. Correct physics:

```
TVG_gain(i) = range_m^(spreading_factor/10) × 10^(α × range_m / 10)
range_m = (i / sample_rate × sound_speed) / 2
```

Two entry points:
- `apply_tvg_correction(samples: &[u16], params) → Vec<f32>` — per-ping  
- `precompute_tvg_lut(max_samples, tvg_alpha, tvg_beta) → Vec<f32>` — lookup table for the mosaic engine  

**Current shortcoming:** TVG parameters are fixed at a single global value. There is no transducer-specific noise floor detection that would allow GT54 (800kHz, high absorption) to use different α than GT56 (455kHz, lower absorption).

---

### 1.5 Mosaic Engine — Geo-Referenced Path (`mosaic/engine.rs`)

`build_mosaic()` is the correct, physics-aware rendering path. Key operations:

**Slant-Range Correction (SRC):**  
Present and correct:
```rust
let slant_m = (ground_m * ground_m + depth * depth).sqrt();
let sample_pos = slant_m / DEFAULT_M_PER_SAMPLE;  // 0.01 m/sample
```
Ground distance is walked from 0 → max_swath_m. For each step, the true slant range is computed from depth + horizontal distance, then the sample index is found by `slant_m / 0.01`. Linear interpolation between sample[base] and sample[base+1] gives the intensity at that ground position.

**Trapezoidal Interpolation:**  
Consecutive ping pairs are iterated as `windows(2)`. Track position is interpolated `0..=track_steps` along the GPS track. Cross-track is interpolated `0..=cross_steps` across the swath. This fills both the track-direction gaps and the angular beam coverage.

**Gaussian Alpha Feathering:**  
Each projected pixel receives a Gaussian weight centered at 60% of the swath (the acoustic "sweet spot" for SideVü). Edge samples at <5% and >95% of swath receive a linear fade. This reduces the bright far-range artifact common in raw per-ping renderings.

**Histogram Normalization:**  
Per-channel P2/P98 stretch computed across all pings before rendering. Output is normalized to `[0.0, 65535.0]` f32.

**Known issue:** The `mosaic/projection.rs` file contains an older `project_pings_to_grid()` function that uses `0.01 m/sample` geometry but lacks SRC, TVG LUT, and trapezoidal interpolation. Both paths remain in the codebase. The `outputs.rs` primary mosaic path (called from the Tauri command) calls `render_sidescan_stitched()` which is a **waterfall-style PNG**, not the geo-referenced grid engine. The `build_mosaic()` path feeds MBTiles/KML but is NOT the default "mosaic_combined.png" output. **This is the most important architectural disconnect in the codebase.**

---

### 1.6 Output Paths

| Output | What Runs | SRC | TVG | Geo-referenced |
|--------|-----------|-----|-----|----------------|
| `mosaic_combined.png` | `render_sidescan_stitched()` | ✗ (waterfall layout) | ✓ (simple) | ✗ |
| `waterfall_chN.png` | `render_waterfall_channel()` | ✗ | ✓ | ✗ |
| `mosaic_geo.mbtiles` | `build_mosaic()` + `export_mbtiles()` | ✓ | ✓ | ✓ |
| `.kmz` | `build_mosaic()` + `export_kmz()` | ✓ | ✓ | ✓ |
| `.mp4/.gif` | `render_enhanced_waterfall()` | ✗ | ✓ | ✗ |

**MBTiles compression:** Currently `PNG + SQLite` (uncompressed PNG blobs). No zstd tile compression. Tile PNG bytes are produced per-tile and inserted directly. This means large surveys produce large .mbtiles files.

---

### 1.7 Nauticuvs Curvelet Library (`nauticuvs-publish/`)

The curvelet crate is a standalone Rust library with forward and inverse curvelet transforms. Key facts:
- Uses `ndarray` f32 arrays internally — **correct Float32 operations**  
- Window functions: Hann, Tukey, Gaussian  
- No TVG integration or transducer-specific parameters  
- `curvelet_denoise` flag in `PipelineOptions` triggers the denoising pass  
- Applied in `outputs.rs` via the `denoised_cache: BTreeMap<u32, GrayImage>` — decodes to u8 grayscale for the cache, loses the f32 precision of the transform output before it reaches the PNG encoder  
- The curvelet path is **optional and off by default** in the UI

---

## Part II — Gap Analysis vs. the Prosumer Vision

### 2.1 The Vision (from the three task specifications)

**Task 1: HeuristicProbe trait**  
> Analyze first 10MB; detect bit-depth (8 vs 12 bit); classify Port/Starboard/Down by signal variance; detect channel flips.

**What we have:** ~75% there.  
- `FileProbe` struct + `probe_file()` exists and classifies channels  
- `ChannelDiscovery` does full signal profiling and flip detection  
- **Gap:** No formal `HeuristicProbe` trait (just structs and functions)  
- **Gap:** 12-bit detection is absent. Gen1 = 8-bit, UHD/UHD2 = 16-bit i16 (treated uniformly — 12-bit packed native formats from some firmware variants are not handled)  
- **Gap:** The probe does not enforce the 10MB cap — it reads the whole file  
- **Gap:** Signal variance classification is entropy-based (good), but does NOT weight variance by range-band (near vs. mid vs. far swath), which would improve classification on weak-signal captures  

---

**Task 2: Curvelet Float32 + transducer-adaptive TVG**  
> Float32 operation; TVG curve adaptive to noise floor of GT54 vs GT56.

**What we have:** 30% there.  
- TVG is Float32 ✓  
- Curvelet is Float32 internally ✓  
- **Gap:** TVG is NOT transducer-adaptive. A single global spreading factor + absorption coefficient is used for all transducers. GT54 (800kHz) has ~2–3× higher absorption loss per meter than GT56 (455kHz). Using the same α distorts both.  
- **Gap:** The curvelet denoised cache converts to u8 `GrayImage` before re-using — losing 8 bits of dynamic range. The curvelet output (f32 coefficients after inverse transform) must stay f32 until final 8-bit colormap quantization.  
- **Gap:** No noise floor measurement. The "noise floor" should be computed per-channel as the median amplitude of the first few samples (pre-first-bottom-return zone), then used to set the TVG start_sample and the soft-threshold in the curvelet denoiser.  

---

**Task 3: Slant Range Correction (SRC) + bilinear resampling**  
> First-bottom-return as input; resample SideVü pings into geographic 2D grid; bilinear interpolation to remove smearing.

**What we have:** 70% there.  
- SRC is implemented in `build_mosaic()`: `slant_m = √(ground² + depth²)` ✓  
- Bilinear interpolation between sample[i] and sample[i+1] along slant ✓  
- Track interpolation (trapezoidal between ping pairs, filling spatial gaps) ✓  
- **Gap:** The "first bottom return" is not being used as the SRC reference. Depth comes from the `depth_m` field in the ping metadata (a separate echo sounder reading). For SideVü pings arriving before the echo sounder updates, depth can be stale by 1–3 pings. A `find_first_bottom_return()` function that detects the amplitude inflection point within the SideVü sample array itself would produce a more accurate, per-ping depth estimate.  
- **Gap:** `build_mosaic()` is NOT connected to the primary `mosaic_combined.png` output. The default mosaic PNG comes from `render_sidescan_stitched()`, which is a simple waterfall strip — pixels placed by ping-row index, not by geographic coordinate. The geo-referenced engine is only exercised by MBTiles/KML outputs.  
- **Gap:** MBTiles tile compression is Deflated (zlib). Should be zstd for 30–50% better compression ratio at similar speed on high-frequency texture data.  

---

**Gap: Empirical Gain Normalization (EGN)**  
> The vision referenced EGN as a beam-pattern flattener.

**What we have:** 0%.  
The per-channel P2/P98 histogram stretch is a global intensity rescale, not EGN. EGN computes a **mean amplitude profile** across all pings at each range-bin (sample index) across the swath — the per-bin average represents pure transducer beam pattern. Dividing each ping by this profile "flattens" the characteristic dark center / bright mid-range / dark far-range gradient caused by the transducer's radiation pattern. This is what makes the FishTec/Humminbird-style output look uniformly textured all the way to the far range.

---

### 2.2 What We're Doing Well

| Strength | Detail |
|----------|--------|
| Self-healing parser | Multi-magic, generation-aware, CRC-tolerant, healing discovery cache |
| Channel flip detection | Nadir-gap edge comparison on first 100 pings, auto-reversal |
| Data-driven channel classification | Entropy + nadir-gap, no header dependency |
| Trapezoidal track interpolation | Fills sub-meter gaps between GPS pings |
| Gaussian beam feathering | Reduces bright-edge artifact at far range |
| Float32 pipeline (engine) | TVG → normalized f32 → grid accumulation |
| Nadir mode tristate | Stitch / Fill (downscan) / Raw, all wired end-to-end |
| SRC in geo path | Correct slant-range geometry |

---

### 2.3 What to Reject or Rethink from the Vision

**"Bilinear interpolation for ping resampling"** — Already better than bilinear. The trapezoidal interpolation between ping pairs is superior to simple bilinear resampling of individual pings because it accounts for the boat's motion between pings, not just the sample spacing within a single ping. Keep what we have.

**"12-bit detection"** — Garmin RSD does not use 12-bit packed format. Consumer/prosumer Garmin units output either 8-bit (Gen1) or 16-bit signed (UHD/UHD2). The confusion comes from Humminbird's Solix/Apex which can record 12-bit. For Garmin specifically, the real differentiation is 8-bit vs near-12-bit effective dynamic range within the i16 samples (UHD samples typically span 0–4095 despite being stored in i16). Adding a check for `max_sample_value < 4096` would correctly fingerprint these as "12-bit range" without needing bit-unpacking.

**"First Bottom Return as SRC input"** — Correct in principle but the current metadata `depth_m` is already a smoothed first-bottom reading from the echo sounder. In practice, using the in-ping amplitude inflection adds complexity with marginal gain unless operating in very soft or layered bottoms where the echo sounder and the SideVü first return disagree. **Recommended**: implement as an optional refinement toggled by a flag, not as a mandatory replacement of the existing depth_m path.

---

## Part III — Marching Orders

### Priority Order

```
P0 — Connect build_mosaic() to the primary mosaic PNG output  (architectural correctness)
P1 — EGN: beam pattern flattener                             (biggest visual impact)
P2 — Transducer-adaptive TVG (GT54 vs GT56 noise floor)      (signal accuracy)
P3 — Curvelet: keep f32 through to colormap                  (precision fix)
P4 — MBTiles zstd tile compression                           (output size reduction)
P5 — First-bottom-return per-ping depth refinement           (SRC accuracy)
P6 — Formal HeuristicProbe trait + 10MB cap                  (architecture hygiene)
```

---

### Task A — P0: Connect `build_mosaic()` to mosaic PNG output

**File:** `src-tauri/src/outputs.rs`  
**What to do:**  
The function `write_mosaic_per_channel()` currently renders `render_sidescan_stitched()` — a waterfall strip with no geographic layout. Add a second mosaic output path that calls `build_mosaic()` from `mosaic/engine.rs`, runs `MosaicGrid::to_rgbimage()`, and saves as `mosaic_geo_chNN.png`. Keep the existing waterfall strip as a fast preview; add the geo-referenced render as the high-quality mosaic option.

**Acceptance criteria:**  
- `mosaic_geo_chNN.png` opens in any GIS viewer at correct lat/lon bounds  
- Ping features align geographically with GPS trackline  
- Scale bar matches known distances  

---

### Task B — P1: Empirical Gain Normalization (EGN)

**New file:** `src-tauri/src/egn.rs`  
**What to do:**  

```rust
/// Compute per-range-bin mean amplitude across all pings for a single channel.
/// Returns Vec<f32> of length max_samples — the beam pattern profile.
pub fn compute_beam_profile(pings: &[&Ping]) -> Vec<f32> {
    let n = pings.iter().map(|p| p.samples.len()).max().unwrap_or(0);
    let mut sum = vec![0.0f32; n];
    let mut count = vec![0u32; n];
    for ping in pings {
        for (i, &s) in ping.samples.iter().enumerate() {
            sum[i] += s as f32;
            count[i] += 1;
        }
    }
    sum.iter().zip(&count).map(|(&s, &c)| if c > 0 { s / c as f32 } else { 1.0 }).collect()
}

/// Apply EGN: divide each sample by the beam profile for that range bin.
/// Clamp to avoid division-by-near-zero in the nadir zone.
pub fn apply_egn(samples: &[u16], profile: &[f32]) -> Vec<f32> {
    samples.iter().enumerate().map(|(i, &s)| {
        let norm = profile.get(i).copied().unwrap_or(1.0).max(1.0);
        s as f32 / norm
    }).collect()
}
```

**Integration point:** In `mosaic/engine.rs` `project_channel()` closure, call `apply_egn(ping.samples, &beam_profile)` after TVG correction, before intensity normalization.  

**Acceptance criteria:**  
- The characteristic dark-center / bright-midrange gradient visible in the second screenshot is eliminated  
- A uniform sandy bottom appears at consistent intensity across the full swath  
- Rocky/structured returns still show contrast relative to the normalized baseline  

---

### Task C — P2: Transducer-adaptive TVG

**File:** `src-tauri/src/video_enhanced/tvg.rs`  
**What to do:**  

Add a `TransducerProfile` enum derived from the file's generation + channel frequency tier:

```rust
#[derive(Clone, Copy)]
pub enum TransducerProfile {
    /// GT54, GT56-UHD-SW or equivalent 800 kHz sidescan
    HighFreq800kHz,
    /// GT52, GT54 455 kHz or UHD standard frequency
    MidFreq455kHz,
    /// Classic GT20/GT22, CHIRP dual — use conservative defaults
    Classic,
}

impl TransducerProfile {
    /// Freshwater absorption coefficient in dB/m at this frequency.
    pub fn absorption_db_per_m(self) -> f32 {
        match self {
            Self::HighFreq800kHz => 0.28,
            Self::MidFreq455kHz  => 0.11,
            Self::Classic        => 0.08,
        }
    }
    /// Recommended spreading exponent (accounts for near-field cylindrical spreading in shallow water).
    pub fn spreading_factor(self) -> f32 {
        match self {
            Self::HighFreq800kHz => 25.0,
            Self::MidFreq455kHz  => 20.0,
            Self::Classic        => 18.0,
        }
    }
}
```

Derive the profile at the `ChannelProfile` level in `channel_discovery.rs` using `FrequencyTier::Detail` → 800kHz, `FrequencyTier::Context` → 455kHz, Gen1Classic → Classic.

Pass `TransducerProfile` into `precompute_tvg_lut()` and `apply_tvg_correction()`.

**Acceptance criteria:**  
- GT54 captures show no over-brightened far-range returns (sign of too-low α)  
- GT56 captures show no artificially dark far-range (sign of too-high α)  
- Both can be independently validated by checking that a flat sandy bottom produces a horizontal intensity band in the waterfall  

---

### Task D — P3: Curvelet f32 pipeline fix

**File:** `src-tauri/src/outputs.rs`  
**What to do:**  
Locate the `denoised_cache: BTreeMap<u32, GrayImage>` population. Currently it converts the curvelet f32 output to u8 `GrayImage` for caching. Instead, cache `Vec<Vec<f32>>` (a 2D f32 ping buffer), and convert to u8 **only** in the final colormap application step.

Concretely:
1. Change `denoised_cache` type to `BTreeMap<u32, Vec<Vec<f32>>>` (channel → pings → samples)
2. In `render_sidescan_stitched()`, accept `f32` samples directly and apply colormap as the last step
3. Remove the intermediate `GrayImage` quantization step

**Acceptance criteria:**  
- Shadow detail in weak-signal zones (deep returns, far range) visibly improved  
- No banding artifacts from 8→16 bit conversion  

---

### Task E — P4: MBTiles zstd tile compression

**File:** `src-tauri/src/mosaic/blending.rs`  
**What to do:**  

```rust
// Add to Cargo.toml:
// zstd = "0.13"

fn compress_tile(png_bytes: &[u8]) -> Vec<u8> {
    zstd::encode_all(std::io::Cursor::new(png_bytes), 3).unwrap_or_else(|_| png_bytes.to_vec())
}
```

Replace the direct PNG insert:
```rust
// Before:
conn.execute("INSERT INTO tiles ... VALUES (?, ?, ?, ?)", params![zoom, col, row, png_bytes])?;
// After:
let tile_data = compress_tile(&png_bytes);
conn.execute("INSERT INTO tiles ... VALUES (?, ?, ?, ?)", params![zoom, col, row, tile_data])?;
```

Also update the metadata `format` entry from `'png'` to `'webp'` or keep as `'png'` but add a custom metadata key `tile_compression = 'zstd'` — required for compatible readers to know they must decompress.  

> **Note:** Standard MBTiles spec only formally supports png/jpg/webp tiles without extra compression. The zstd tile compression requires a consuming app to handle it (e.g. when loading into Leaflet via custom TileLayer). If you need standard MBTiles compatibility, use WebP (75% quality) instead of PNG+zstd — WebP gives similar file size reduction and is natively supported by the spec.  
> **Recommendation:** Add a `tile_format` option: `"png"` (spec-compatible default), `"webp"` (best compatibility+size), `"png+zstd"` (fastest for internal tools).

---

### Task F — P5: Per-ping first-bottom-return depth refinement

**New file:** `src-tauri/src/src_refinement.rs`  
**What to do:**  

```rust
/// Detect the first significant amplitude return in a SideVü ping.
/// Returns the sample index of the first-bottom-return.
///
/// Method: walk from near range toward far. The nadir zone is intentionally
/// skipped. Find the first sample where amplitude rises above the noise_floor
/// and stays elevated for at least `min_run` consecutive samples.
pub fn find_first_bottom_return(
    samples: &[u16],
    nadir_gap: usize,
    noise_floor: f32,
    min_run: usize,
) -> Option<usize> {
    let start = (nadir_gap + 5).min(samples.len());
    let mut run = 0;
    for i in start..samples.len() {
        if samples[i] as f32 > noise_floor * 1.5 {
            run += 1;
            if run >= min_run {
                return Some(i - min_run + 1);
            }
        } else {
            run = 0;
        }
    }
    None
}

/// Convert first-bottom-return sample index to depth in meters.
pub fn sample_to_depth_m(fbr_sample: usize, m_per_sample: f64) -> f64 {
    fbr_sample as f64 * m_per_sample
}
```

**Integration point:** In `mosaic/engine.rs` `project_channel()`, after retrieving `ping_a.depth_m`, optionally compute `fbr_depth` and use `fbr_depth.unwrap_or(ping_a.depth_m)` as the SRC depth.  
Guard behind a `MosaicConfig` flag: `pub use_fbr_depth: bool` (default `false`).

---

### Task G — P6: Formal HeuristicProbe trait + 10MB cap

**File:** `src-tauri/src/channel_discovery.rs`  
**What to do:**  

```rust
pub trait HeuristicProbe {
    /// Analyze at most `probe_bytes` of raw file data.
    /// Returns a ProbeReport without a full parse.
    fn probe(&self, data: &[u8], probe_bytes: usize) -> ProbeReport;
}

pub struct ProbeReport {
    pub generation: RsdGeneration,
    pub bit_depth: BitDepth,       // U8, I16, I16_12BitRange
    pub channels: Vec<ChannelProbe>,
    pub confidence: f32,
}

pub struct ChannelProbe {
    pub id: u32,
    pub archetype: SignalArchetype,
    pub spatial_role: SpatialRole,
    pub is_flipped: bool,
    pub nadir_gap_samples: usize,
    pub effective_bit_depth: BitDepth,
}
```

`impl HeuristicProbe for GarminRSDParser` — delegates to `probe_file()` with `data[..10MB.min(data.len())]`.

---

## Part IV — Target State

The image attached (FishTec HD Fishing Charts, first screenshot) represents the prosumer target:

| Feature | Current state | Target state |
|---------|--------------|--------------|
| Uniform bottom texture | Bright-center gradient visible | EGN flattens beam pattern globally |
| Track-line transitions | Visible seam lines at track boundaries | Gaussian feathering + EGN produce invisible blends |
| Nadir zone | Black strip, blended with adjacent pixels | Downscan fill OR clean stitch with no dark strip |
| Geographic accuracy | Geo path exists but disconnected from PNG | `build_mosaic()` → PNG, validated against GPS overlay |
| Dynamic range | P2/P98 stretch globally | Log-compressed TVG-corrected EGN-normalized f32 pipeline |
| Depth contours overlay | Not implemented | Label using depth_m from ping stream per tile |
| Tile output size | ~2× larger than needed | WebP or zstd-compressed tiles |
| Transducer support | GT54/GT56 treated identically | Frequency-tier → TransducerProfile → adaptive TVG |

The second screenshot (our best output, `mosaic_combined.png`, 35263×23731, 81.8MB) shows the characteristic issues this plan addresses:
- Horizontal striping: beam-pattern gradient not removed (EGN would fix this)  
- Visible ping-row boundaries at turns: trapezoidal interpolation in `build_mosaic()` handles this but that path doesn't feed the PNG output yet (Task A)  
- Generally good structural detail: the parser, channel flip correction, and waterfall rendering are working

---

## Implementation Sequence for an AI Coder

Execute in this exact order — each task has minimal dependencies on the previous:

```
1. Task E (MBTiles zstd/WebP)  — isolated, no dependencies, 2-hour task
2. Task D (Curvelet f32 fix)   — isolated, improves existing denoising immediately
3. Task B (EGN module)         — new file, plug into engine.rs; biggest visual payoff
4. Task C (Adaptive TVG)       — builds on TVG module, add TransducerProfile enum
5. Task A (Connect build_mosaic → PNG) — integrates engine.rs into outputs.rs render path
6. Task F (FBR depth refinement) — optional refinement, adds MosaicConfig flag
7. Task G (HeuristicProbe trait) — architecture hygiene, last
```

Each task should be implemented with a matching integration test using the files in `test files/` (`.rsd` captures are present in the workspace). The batch quality report (`_batch_mosaic_quality.csv` in `testoutputs/`) provides a baseline: after each task, re-run the batch and verify the quality metrics improve.

---

*End of White Paper — SonarSniffer Rust v1.0 Technical Audit*
