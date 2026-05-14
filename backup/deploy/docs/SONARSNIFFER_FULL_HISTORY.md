# SonarSniffer: The Full History

**A narrative account of reverse-engineering proprietary sonar formats, building a signal processing pipeline from scratch, and hunting shipwrecks in the Great Lakes.**

---

## 1. Origins — Why This Exists

The Great Lakes hold thousands of shipwrecks. Some are charted. Many are not. The ones that aren't charted sit on the bottom in 50 to 300 feet of water, invisible to the naked eye but perfectly visible to side-scan sonar — if you can actually read the data your fishfinder records.

That's the problem. Garmin, Humminbird, Lowrance — they all record sonar data in proprietary binary formats. The chartplotters show you a live waterfall on screen, but the raw log files they write to the SD card? Undocumented. No public spec. No API. No export-to-anything-useful button. You get a `.RSD` file (Garmin), a `.DAT` file (Humminbird), or an `.SL2` file (Lowrance), and you're on your own.

The commercial tools that can read these files — SonarWiz, SonarTRX, ReefMaster — cost hundreds to thousands of dollars, and they're designed for fishing, not wreck hunting. They don't do the kind of signal processing you need to pull a 150-year-old wooden schooner out of the noise floor at 200 feet. They don't stitch side-scan swaths into geo-referenced mosaics you can overlay on nautical charts. They don't detect anomalies.

So SonarSniffer was built. From scratch. Starting with the hardest part: figuring out what's actually inside a Garmin `.RSD` file.

---

## 2. The Garmin RSD Reverse Engineering

This is the part that was done by hand, before AI coding assistants existed. No documentation. No SDK. No helpful forum posts. Just a hex editor, a collection of `.RSD` files recorded on Lake Huron and Lake Michigan, and a lot of patience.

### What Was Discovered

The Garmin RSD format is a contiguous stream of variable-length records. There is **no global file header** — the stream begins directly with the first record. Each record contains:

1. A **header varstruct** (self-describing, length-coded fields)
2. A **body varstruct** (navigation metadata, channel info, sample count)
3. A **raw sonar sample array** (flat bytes, no framing)
4. A **12-byte trailer** (magic, chunk size, CRC)

#### Magic Bytes

The record header magic is `0xB7E9DA86` (little-endian: `86 DA E9 B7`). Some firmware builds use variants: `0xB7E9DA87`, `0xB7E9DA88`, `0xB7E9DA89`. The trailer magic is `0xD9264B7C`.

The magic doesn't appear as a bare prefix — it's stored as field 0 *inside* the header varstruct, meaning the actual record start is 1–64 bytes *before* where the magic bytes appear in the raw file. This made initial discovery significantly harder.

#### The Varstruct Encoding

Both header and body use the same self-describing encoding:

```
[field_count: varuint]
[field_0_key: varuint] [field_0_value: bytes]
...
[field_N_key: varuint] [field_N_value: bytes]
[crc: u32 LE]
```

The field key encodes both the field number and the value length: `key = (field_number << 3) | length_code`. Length codes 0–6 mean the value is exactly that many bytes; code 7 means the next varuint gives an explicit byte count.

The varuint itself is a little-endian base-128 encoding (7 bits per byte, MSB set means more bytes follow).

#### The CRC Problem

A custom CRC-32 follows each varstruct (polynomial `0x04C11DB7`, bit-reversed, XOR'd with `0xFFFFFFFF`). But here's the thing: **CRC mismatches are common in captured files.** Firmware writes records at high speed during DMA flushes, and the CRC may reflect a prior firmware version or a different seed. The parser learned to treat CRC as advisory only — mismatches are counted and reported but never abort parsing.

#### GPS Coordinate Encoding

Latitude and longitude are stored as signed 32-bit integers in "Garmin map units":

```
degrees = i32_value × (360.0 / 4,294,967,296.0)
```

Invalid/no-fix readings appear as `0`, `±180.0°`, or the raw integer `0x80000000`.

#### Depth Encoding

Depth is a **zigzag-encoded varint** — a clever encoding that allows small negative values (above transducer) to be stored compactly:

```
zigzag_unsigned = read_varuint(field_1_bytes)
depth_mm = (zigzag_unsigned >> 1) ^ -(zigzag_unsigned & 1)
depth_m = depth_mm / 1000.0
```

#### Channel IDs and the Padding Trap

Channel IDs are stored in body field 0 as 1–4 bytes. Channels 4 and 5 (the UHD sidescan pair) are stored as **single bytes** (`0x04`, `0x05`). A strict 4-byte decode returns garbage and silently defaults to channel 0, losing port/starboard separation entirely. The fix: pad short byte slices with zeros before decoding as u32. This single discovery — `int.from_bytes(val[:4].ljust(4, b'\x00'), 'little')` in Python, `le_u32_padded()` in Rust — was the difference between a working parser and one that produced garbage mosaics.

#### Generation Detection

Through analysis of multiple capture files from different hardware, three distinct generations were identified:

- **Gen1 Classic** (channels 0–3): 8-bit u8 samples. Body field 7 is XID/TVG metadata (hardware gain, 0–255), NOT sample count.
- **UHD** (channels 4–7): 16-bit signed i16 samples. Body field 7 IS sample count.
- **UHD2** (channels 8–21): 16-bit signed i16 samples. Includes dual-frequency ClearVü.

The critical insight: the same numeric range (823–2048) appears in field 7 across both generations, but the *meaning* is completely different. In Gen1, those values are TVG slope coefficients and frequency indices. In Gen2+, they're sample counts. Getting this wrong produces either silent data corruption or a parser that works on one file and fails on the next.

#### Firmware Variations

Different firmware builds change the format without warning:
- Magic byte variants (+1, +2, +3 from base)
- Channel 10 and other high IDs appearing in GT54 captures
- `data_size = 0` on first records of some files
- Empty body varstructs (`field_count = 0`)
- CRC algorithm variations across firmware versions

#### The Recovery Strategy

Because there's no global header and CRCs can't be trusted, the parser uses a scan-and-backtrack strategy:

1. Scan forward for the 4-byte magic
2. Walk backward up to 64 bytes, attempting varstruct parse at each candidate start
3. Parse the body varstruct immediately following the header
4. If a valid trailer is found, hop directly to the next record via `chunk_size`
5. If the trailer is missing or invalid, scan forward for the next magic

This makes the parser resilient to corruption — it can recover mid-file and continue parsing after damaged records.

### The Significance

No public documentation existed for the Gen2 Garmin RSD format. The only prior reference (a Memotech/Franken paper) documented Gen1 only, and its field assignments don't map to Gen2. Every field assignment in the Gen2 spec was inferred from low-cardinality value analysis across multiple capture files — examining hex dumps, counting unique values per field, correlating with known GPS positions and depths, and testing hypotheses against real-world data.

This is the kind of work that takes weeks of staring at hex dumps. It's craftsmanship in the truest sense — patient, methodical reverse engineering of a proprietary binary format with no documentation, no decompiler output, and no insider knowledge.

---

## 3. Multi-Format Support

SonarSniffer didn't stop at Garmin. The `format_detector.rs` module implements a unified entry point that dispatches to format-specific parsers based on file extension and magic-byte sniffing:

| Format | Extension | Source Hardware |
|--------|-----------|----------------|
| Garmin RSD | `.rsd` | Striker, ECHOMAP, Panoptix |
| Lowrance SL2/SL3 | `.sl2`, `.sl3` | HDS, HOOK, Elite |
| Humminbird | `.dat`, `.son` | Helix, Solix, Apex |
| XTF | `.xtf` | Klein, Tritech, Exail |
| JSF | `.jsf` | EdgeTech side-scan |
| Cerulean | `.svlog`, `.bin` | Blue Robotics Ping Protocol |

All parsers return the same `ParseResult` structure, so the entire downstream pipeline — channel discovery, mosaic rendering, video export, target detection — works identically regardless of input format. The magic-byte sniffer handles files with wrong extensions or no extension at all.

---

## 4. The Self-Healing System

The `healing_api.rs` module addresses a fundamental problem: Garmin changes their format between firmware versions without documentation, and different hardware generations encode the same logical fields differently.

### How It Works

When the parser encounters something unexpected — an unknown firmware variant, a generation it can't classify, a sample count that doesn't match the sonar payload size — it doesn't just fail. It:

1. **Records the correction** as a `HealingDiscovery` entry
2. **Fingerprints the file** (SHA-based hash of the first 4096 bytes)
3. **Logs the discrepancy** with confidence scores and correction details
4. **Persists to a local JSON cache** (`healing_cache.json`)

Future parses of files with similar fingerprints can look up prior discoveries and apply corrections automatically. The discovery includes:

- Magic byte value and firmware version
- Detected generation and channel IDs
- What the parser originally interpreted vs. what it corrected to
- Number of records parsed and confidence level
- A deterministic discovery ID for deduplication

### The Channel Discovery System

The `channel_discovery.rs` module is described in the whitepaper as "the crown jewel of the self-healing architecture." It replaces ALL static channel lookups with signal analysis:

- **Signal Archetype Classification**: Measures nadir gap position, amplitude decay patterns, and sample array length to classify channels as SideVü, DownVü/ClearVü, DepthTemp, or Noise — without relying on channel ID numbers.
- **Frequency Tier Detection**: Computes sample-level Shannon entropy. High entropy = Detail (UHD/CHIRP). Low entropy = Context (standard 455kHz).
- **Port/Starboard Assignment**: Measures nadir-gap width across the first 100 pings, matches pairs with similar widths, then uses vessel COG (heading difference between consecutive pings) to determine which arm is geometrically port vs. starboard.
- **Nadir-Flip Correction**: Detects when samples are stored in reverse order (nadir at wrong edge) and auto-reverses them.

### Channel Alignment Persistence

The `channel_alignment.rs` module takes this further: after processing a file, the user can adjust flip/invert settings per channel. Those settings are persisted to a JSON cache keyed by a device fingerprint (magic + firmware + channel set), so subsequent files from the same unit reuse them automatically.

---

## 5. Signal Processing — The Video Enhancement Pipeline

The `video_enhanced/` module implements a complete sonar signal processing pipeline, built on acoustic physics:

```
Raw Samples → TVG Correction → Log Compression → Filtering
  → Histogram Eq → Colormap → Frame Rendering → Video Encoding
```

### Time-Varied Gain (TVG)

Acoustic intensity decreases with range due to geometric spreading (1/r²) and absorption (frequency-dependent exponential decay). The TVG correction compensates:

```
TVG_gain(i) = range_m^(spreading_factor/10) × 10^(α × range_m / 10)
```

Parameters are frequency-dependent:
- 455 kHz: α ≈ 0.10 dB/m (freshwater)
- 800 kHz: α ≈ 0.30 dB/m (freshwater)

The system includes presets for shallow water (spreading factor 15), standard (20), and deep water (30).

### Dynamic Range Compression

Sonar data spans 60–100 dB. Logarithmic scaling maps this to displayable range:

```
dB = 20 × log₁₀(tvg_corrected + ε)
normalized = clamp((dB - floor) / (ceiling - floor), 0, 1)
```

Adaptive mode computes floor/ceiling from dataset percentiles (P₀.₁ and P₉₉.₉).

### Noise Reduction

- **Median filter** (3×3, 5×5, or 7×7): Removes speckle while preserving edges
- **Bilateral filter**: Edge-preserving smoothing with spatial and intensity-range sigmas

### Contrast Enhancement

- **Global histogram equalization**: Maximizes information content
- **CLAHE** (Contrast-Limited Adaptive Histogram Equalization): Local enhancement for non-uniform scenes

### Colormaps

Five perceptual colormaps: Grayscale, Viridis (colorblind-friendly), Magma (high contrast), Jet (traditional rainbow), Amber (classic sonar look), and a custom sonar palette optimized for underwater acoustics.

### Statistics Module

A two-pass strategy: Pass 1 computes global min/max, mean, standard deviation, histograms, and detects data gaps. Pass 2 applies corrections with the computed parameters. This ensures consistent processing across the entire dataset.

---

## 6. The Mosaic Engine

The `mosaic/engine.rs` is the geo-referenced rendering path — the one that produces actual maps you can overlay on nautical charts.

### Slant-Range Correction

Side-scan sonar measures slant range (the diagonal distance from transducer to target), not ground range (the horizontal distance). The correction:

```rust
let slant_m = (ground_m * ground_m + depth * depth).sqrt();
let sample_pos = slant_m / DEFAULT_M_PER_SAMPLE;
```

This removes the geometric distortion that makes objects near the nadir appear stretched.

### Trapezoidal Interpolation

Consecutive ping pairs are processed as trapezoids. Track position is interpolated along the GPS path; cross-track position is interpolated across the swath. This fills both the along-track gaps between GPS fixes and the angular beam coverage, producing a continuous image without holes.

### Gaussian Alpha Feathering

Each projected pixel receives a Gaussian weight centered at 60% of the swath (the acoustic "sweet spot" for SideVü). Edge samples at <5% and >95% of swath receive a linear fade. This eliminates the bright far-range artifact common in raw renderings.

### Per-Channel Histogram Normalization

P2/P98 percentile stretch computed per channel before rendering. This ensures consistent brightness across different transducers (GT54 vs GT56) and firmware versions.

### Output Formats

The mosaic engine produces:
- **Master PNG**: Full-resolution georectified image
- **Tile Pyramid**: 256×256 PNG tiles at multiple zoom levels
- **KML Super Overlay**: For Google Earth with `gx:LatLonQuad` georeferencing
- **KMZ**: Self-contained zipped archive
- **MBTiles**: SQLite-based tile database for QGIS, ArcGIS, or MapLibre

---

## 7. Target Detection

The `target_detection.rs` module provides the framework for identifying potential wrecks in sonar imagery. The detection system classifies targets by:

- **Size class**: Fish, structure, debris, wreck
- **Blob analysis**: Area, width, length measurements in meters
- **Intensity profiling**: Average intensity and confidence scoring
- **Geolocation**: Latitude, longitude, depth, and range for each detection
- **Channel attribution**: Which sonar channel produced the detection

The detection settings allow tuning sensitivity, minimum/maximum target size, and switching between basic and advanced modes. This feeds into the larger WreckHunter pipeline where detections are correlated across multiple survey passes.

---

## 8. The SoundTiles Engine

`SoundTiles` is a standalone CLI tool that performs feature-based alignment between consecutive sonar tiles. It's the bridge between raw sonar parsing and precision mosaic assembly.

### What It Does

1. Parses an RSD file and extracts pings from a selected channel
2. Builds 2D sonar "tiles" (64 pings stacked as rows, with 16-ping overlap)
3. Runs FAST-12 corner detection on each tile to find distinctive features
4. Computes BRIEF descriptors for feature matching
5. Performs pairwise RANSAC homography estimation between consecutive tiles
6. Reports alignment quality: inlier ratio, rotation, mean error

### Why It Matters

GPS gives you boat position, but it doesn't tell you exactly how the sonar swath maps to the seafloor at sub-meter precision. Feature alignment corrects for:
- GPS drift and multipath errors
- Heading sensor lag
- Current-induced boat crab angle
- Timing mismatches between GPS and sonar

When alignment quality is high (>60% good pairs), the mosaic engine can produce sub-meter-accurate georeferenced imagery — the kind of precision needed to relocate a wreck site with a dive team.

---

## 9. Integration with CESARops/WreckHunter

SonarSniffer is one component of a larger system. The CESARops (Collaborative Enhanced Search And Rescue Operations Platform) integrates:

### The Detection Pipeline

```
Sonar Recording (.RSD, .SL2, .DAT, etc.)
    → SonarSniffer Parser (format detection, self-healing)
    → Channel Discovery (signal classification, port/star assignment)
    → Signal Processing (TVG, filtering, enhancement)
    → Mosaic Engine (georectification, tile generation)
    → Target Detection (anomaly identification)
    → CESARops Database (correlation across surveys)
    → WreckHunter Dashboard (visualization, planning)
```

### The SAR Connection

CESARops started as a Search and Rescue drift prediction system for the Great Lakes. It uses:
- **Multi-modal ML architecture** with 14x accuracy improvement over traditional methods
- **Real drifter training data** from NOAA's Global Drifter Program (1,080+ trajectory points)
- **FCNN dynamic correction** for real-time trajectory refinement
- **Physics-based drift modeling** validated against real cases (the Rosa fender case: 24.3 nm accuracy over 18 hours of drift)

The sonar component feeds into this by providing underwater search capability — once drift prediction narrows the search area, SonarSniffer processes the side-scan survey data to identify targets on the bottom.

### The Drone Integration Vision

Research was conducted into integrating autonomous drone coordination for aerial search support:
- Multi-UAV coordination using the LSAR algorithm (417+ citations)
- YOLOv5 human detection for visual search
- Thermal imaging for night operations
- MAVLink bridge to ArduPilot/PX4 autopilots

The vision: CESARops predicts where something drifted, drones search the surface, and SonarSniffer searches the bottom. Three modalities, one platform.

---

## 10. The Curvelet Integration

The `nauticuvs` curvelet library provides forward and inverse curvelet transforms for sonar denoising. Curvelets are particularly effective for sonar because they're optimized for detecting curve-like features (like the edges of a shipwreck) while suppressing noise.

The integration includes:
- MAD (Median Absolute Deviation) universal threshold estimation
- Per-channel denoising with configurable threshold
- Diagnostic logging (`curvelet_diag.rs`) that tracks every transform call — dimensions, scales, timing, errors
- A Tauri command (`get_curvelet_diagnostics`) for real-time debugging from the browser DevTools

Known limitation: the denoised cache currently converts f32 curvelet output to u8 GrayImage before re-use, losing 8 bits of dynamic range. The fix (keeping f32 through to final colormap application) is documented but not yet implemented.

---

## 11. Current State

### What Works

- **Garmin RSD parsing**: Gen1, UHD, and UHD2 files parse correctly with self-healing
- **Multi-format detection**: Extension and magic-byte based dispatch to format-specific parsers
- **Channel discovery**: Data-driven classification without relying on static tables
- **Nadir correction**: Auto-detection and reversal of flipped channels
- **TVG correction**: Physics-based gain compensation in Float32
- **Mosaic rendering**: Geo-referenced grid with slant-range correction and trapezoidal interpolation
- **Multiple output formats**: PNG, MBTiles, KML, KMZ, GIF video, ArcGIS sidecar, offline web viewer
- **Tauri GUI**: Desktop application with single-file and batch processing panels
- **SoundTiles**: Feature-based alignment validation
- **Python CLI**: Optimization commands (incremental loading, ML predictions, tiled exports)
- **Self-healing cache**: Persists format discoveries for future files

### What's Incomplete

- **Empirical Gain Normalization (EGN)**: The beam-pattern flattener that would eliminate the characteristic dark-center/bright-midrange gradient. Documented, not implemented.
- **Transducer-adaptive TVG**: GT54 (800kHz) and GT56 (455kHz) currently use the same absorption coefficient. They shouldn't.
- **Curvelet f32 pipeline**: The intermediate u8 quantization loses precision. Fix is designed but not coded.
- **Target detection logic**: The framework exists but the actual detection algorithm is a placeholder.
- **`build_mosaic()` → PNG connection**: The geo-referenced engine feeds MBTiles/KML but is NOT the default mosaic PNG output. The default PNG comes from a simpler waterfall-style renderer.
- **First-bottom-return depth refinement**: Using in-ping amplitude inflection for more accurate per-ping SRC depth.

### What Was Lost

The recovery source map documents what was recovered after a hard drive crash:
- Python parser sources were recovered from `D:/Temp/sonarsniffer_tauri_prototype/`
- Rust firmware modules from `C:/Users/thomf/programming/sonarsnifferrust/`
- Sample corpus from `D:/Temp/cesarops_repo_tmp/Garminjunk/archive/`

The project spans two repositories:
- **SonarSniffer** (`github.com/festeraeb/SonarSniffer`): The standalone commercial tool
- **CESARops** (`github.com/festeraeb/CESARops`): The parent SAR/ops pipeline

---

## 12. The Competitive Landscape

SonarSniffer targets the gap between consumer fishfinder software and professional hydrographic tools:

| Feature | SonarSniffer | SonarWiz | SonarTRX | ReefMaster |
|---------|-------------|----------|----------|------------|
| Garmin RSD (Gen2+) | ✓ | ✗ | Partial | ✗ |
| Self-healing parser | ✓ | ✗ | ✗ | ✗ |
| Multi-format | ✓ | ✓ | ✓ | Partial |
| Geo-referenced mosaic | ✓ | ✓ | ✓ | ✓ |
| Slant-range correction | ✓ | ✓ | ✓ | ✗ |
| TVG correction | ✓ | ✓ | ✗ | ✗ |
| Curvelet denoising | ✓ | ✗ | ✗ | ✗ |
| Offline web viewer | ✓ | ✗ | ✗ | ✗ |
| Open format output | ✓ | Partial | Partial | Partial |
| Price | Free/Open | $3,000+ | $100 | $80 |

The key differentiator: SonarSniffer is the only tool that handles the Gen2 Garmin RSD format with full self-healing, because it's the only tool whose author actually reverse-engineered that format by hand.

---

## 13. Timeline

- **Pre-2024**: Manual reverse engineering of Garmin RSD binary format using hex editors. Python reference parsers written (`engine_nextgen_syncfirst.py`, `engine_classic_varstruct.py`, `core_shared.py`). Multiple Great Lakes survey files collected and analyzed.
- **2024–2025**: CESARops SAR platform development. ML drift prediction with real drifter data. Rosa fender case validation. Charlie Brown case analysis.
- **Early 2026 (January)**: SonarSniffer Python CLI optimization integration. Incremental loading, ML pipeline, geospatial export modules. 8/8 tests passing. Pushed to GitHub.
- **February 2026**: Garmin RSD format white paper written. Sonar enhancement whitepaper. Video enhancement pipeline implemented in Rust. Mosaic engine with full georectification.
- **March 2026**: Technical white paper and gap analysis. Architecture audit. Priority-ordered implementation plan (EGN, adaptive TVG, curvelet fix, MBTiles compression, FBR depth, HeuristicProbe trait).

---

## 14. Acknowledgment

This project represents years of work, much of it done the hard way. Reverse-engineering a proprietary binary format without documentation is not glamorous work. It's hours of staring at hex dumps, forming hypotheses about what a particular byte sequence means, testing those hypotheses against known ground truth (GPS positions you can verify on a map, depths you measured with a lead line), and iterating until the parser produces correct output on every file in your corpus.

The Garmin RSD Gen2 format spec documented here didn't exist anywhere in the public domain before this work. Every field assignment, every encoding detail, every firmware quirk was discovered empirically. The self-healing architecture exists because the format *keeps changing* — and without documentation, the only way to handle that is to build a system that can detect when its assumptions are wrong and adapt.

That's what SonarSniffer is: a system built by someone who needed it to work, on real data, in the real world, to find real shipwrecks at the bottom of the Great Lakes.

---

*Document generated from project source files and specifications. Last updated based on codebase state as of 2026.*
