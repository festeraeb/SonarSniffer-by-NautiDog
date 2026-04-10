# Test Files Analysis & Channel Detection Improvements

**Date:** 2026-03-31  
**Status:** Reverse-engineering analysis and improvement recommendations  
**Files Analyzed:** 929 files across GT51, GT54, GT56 transducer test corpus

---

## Executive Summary

Analysis of the test files corpus reveals several critical issues affecting orientation detection and channel identification:

1. **GT54 Orientation Problem**: The parser's `normalize_nadir_direction()` function flips samples at parse time, but the channel discovery module's `nadir_flip_test()` is now redundant and may cause confusion
2. **GT51 Single-Wing Detection**: GT51 asymmetric transducers need special handling for single-side port/starboard identification
3. **GT56 Channel Ambiguity**: UHD2+ firmware can use multiple channel layouts (8-series, 10-series, 14-series) requiring runtime disambiguation
4. **Missing Metadata**: Body field 0 (channel ID) is absent in some GT54 captures, causing all records to default to channel 0

---

## 1. Test Files Catalog

### 1.1 RSD Files by Transducer Type

| File | Transducer | Channels | Records | GPS | Notes |
|------|------------|----------|---------|-----|-------|
| **GT51 (Classic)** |||||
| `Sonar011cvgt51.rsd` | GT51 ClearVü | 4, 6 | ~15K | ✓ | Michigan capture |
| **GT54 (UHD/UHD2)** |||||
| `126SV-UHD2-GT54.RSD` | GT54 UHD2 | 1, 2, 5, 10 | ~9K | ✗ | Test capture, **body field 0 absent** |
| `Holloway.RSD` | GT54 UHD | 4, 5 | ~81K | ✓ | Classic sidescan pair |
| `Sonar000.RSD` | GT54 UHD | 4, 5 | ~170K | ✓ | Full sidescan |
| `CV-54-UHD.RSD` | GT54 ClearVü | 6 | TBD | ? | ClearVü mode |
| `CV-54-UHD2.RSD` | GT54 ClearVü | 10, 11 | TBD | ? | UHD2 ClearVü |
| **GT56 (UHD2+)** |||||
| `126SV-UHD2-GT56.RSD` | GT56 UHD2 | 4-7, 8-11 | TBD | ✗ | Multi-channel |
| `93SV-UHD-GT56.RSD` | GT56 UHD | 993 | 173 | ✗ | Legacy channel ID |
| `25MAR25-0736-01_2/` | GT56 UHD2+ | 10, 11, 12, 13 | TBD | ✓ | **10-series layout** |
| `CV-56-UHD.RSD` | GT56 ClearVü | 6, 18, 20 | TBD | ? | Multi-freq ClearVü |
| `CV-56-UHD2.RSD` | GT56 ClearVü | 10, 11, 18, 20 | TBD | ? | UHD2 ClearVü |

### 1.2 Hidden/Embedded Data Discovered

1. **ArcGIS Layer JSON** (6 files, up to 30MB each)
   - Per-ping attributes: altitude_m, beam_angle_deg, bottom_hardness, channel, depth_ft/m, heading_deg, pitch_deg, roll_deg, sample_count, sequence, timestamp_ms
   - Spatial reference: WKID 4326 (WGS84)

2. **MBTiles Databases** (7 files)
   - SQLite-based map tile storage
   - Sonar mosaic tiles with geographic projection
   - Zoom level pyramids

3. **Firmware Lookup Patterns** (from `firmware_lookup.rs`)
   - Known float identifiers: `8.43, 10.09, 11.79, 24.72, 14.10, 29.01, 40.05, 13.50, 13.30, 13.40, 4.20, 4.10, 6.80, 40.04`
   - XOR masks for string extraction: `0x00, 0x45, 0xC5, 0x80`

4. **Web Viewer Applications** (in `*/viewer/` directories)
   - MapLibre GL-based viewers
   - Embedded GeoJSON ping data
   - Detection markers (fish, baitball, structure, debris, wreck)

---

## 2. GT54 Orientation Detection Issue

### 2.1 Problem Statement

The GT54 test file `126SV-UHD2-GT54.RSD` exhibits incorrect orientation detection due to:

1. **Body field 0 absent**: All records default to channel 0, losing port/starboard separation
2. **Redundant flip logic**: Parser's `normalize_nadir_direction()` flips samples, but `nadir_flip_test()` in channel_discovery.rs is still called
3. **Static map mismatch**: `map_channel_info()` labels don't match actual firmware output

### 2.2 Root Cause Analysis

```rust
// In garmin_rsd_parser.rs line ~480
let channel = le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0);
// ↑ When body field 0 is absent, defaults to channel 0
```

**Evidence from test file:**
- File: `126SV-UHD2-GT54.RSD`
- Expected channels: 1, 2, 5, 10 (from filename and Python parser reference)
- Actual parsed: All records → channel 0
- Reason: Body field 0 not present in varstruct

**Nadir flip redundancy:**
```rust
// channel_discovery.rs line ~1040
fn nadir_flip_test(...) -> bool {
    // This test is now redundant with ParseResult::normalize_nadir_direction()
    // which already flips samples at parse time.
    log.push(format!(
        "ch{}: nadir-flip delegated to parser normalize_nadir_direction()",
        ch_id
    ));
    false  // ← Always returns false, detection skipped
}
```

### 2.3 Recommended Fix

**Option A: Enhance body field 0 recovery**
```rust
// When body field 0 is absent, use channel ID from:
// 1. File preamble metadata block (VS#0 f06)
// 2. Beam type enum (body field 12) + generation enum (body field 6)
// 3. Filename heuristic as last resort
fn recover_channel_from_metadata(bytes: &[u8], pos: usize) -> Option<u32> {
    // Scan backward for preamble metadata block
    // Extract channel IDs from VS#0 f06 encoding
    // Match to current record by timestamp proximity
}
```

**Option B: Remove redundant nadir flip test**
```rust
// Delete nadir_flip_test() entirely - parser already handles it
// Update documentation to clarify single-pass normalization
```

---

## 3. GT51 Channel Breakdown & Alignment

### 3.1 GT51 Architecture

GT51 is an **asymmetric single-wing** transducer:
- **Classic mode**: Channels 0-3 (8-bit u8 samples)
- **ClearVü mode**: Channels 4, 6 (16-bit i16 samples)
- **Nadir position**: At ONE EDGE (not center like GT54/GT56)

### 3.2 Current Detection Logic

```rust
// channel_discovery.rs line ~267-280
// GT51 asymmetric single-wing detection
for p in profiles.iter_mut() {
    if p.archetype == SignalArchetype::SideVu && p.spatial_role == SpatialRole::Unassigned {
        p.spatial_role = match p.nadir_edge {
            NadirEdge::Left  => SpatialRole::SingleSidePort,
            NadirEdge::Right => SpatialRole::SingleSideStarboard,
            _                => SpatialRole::Unassigned,
        };
        // ...
    }
}
```

### 3.3 Issue: Nadir Edge Detection Fails on GT51

**Problem**: The sliding-window nadir classification assumes paired sidescan geometry:

```rust
// channel_discovery.rs line ~640-700
fn classify_nadir_edge_sliding(pings: &[&Ping]) -> (NadirEdge, f32) {
    // Three windows: A (0-15%), B (45-55%), C (85-100%)
    // GT51 single-wing: A or C quiet (edge nadir)
    // GT54/GT56 paired: B quiet (center nadir after flip)
    
    let a_quiet = mean_a < threshold;
    let b_quiet = mean_b < threshold;
    let c_quiet = mean_c < threshold;
    
    let edge = if b_quiet {
        NadirEdge::Center  // ← GT54/GT56 paired
    } else if a_quiet && !c_quiet {
        NadirEdge::Left    // ← GT51 port
    } else if c_quiet && !a_quiet {
        NadirEdge::Right   // ← GT51 starboard
    }
    // ...
}
```

**Issue**: GT51 captures show **both A and C quiet** because:
1. Single-wing transducer has water column on ONE SIDE ONLY
2. After parser flip correction, nadir should be at Left
3. But GT51 starboard configuration has nadir at Right

### 3.4 Recommended Fix

**Add GT51-specific detection:**
```rust
fn detect_gt51_single_wing(profiles: &[ChannelProfile]) -> Option<u32> {
    // GT51 signatures:
    // 1. Channel IDs 0-3 OR (4, 6) pair only
    // 2. No paired sidescan (only 1 SideVü channel)
    // 3. Asymmetric nadir (edge, not center)
    // 4. Classic generation (8-bit samples) OR ClearVü dual-mode
    
    let sidevu_channels: Vec<_> = profiles
        .iter()
        .filter(|p| p.archetype == SignalArchetype::SideVu)
        .collect();
    
    if sidevu_channels.len() == 1 {
        let ch = sidevu_channels[0];
        if ch.channel_id <= 3 || (ch.channel_id == 4 || ch.channel_id == 6) {
            return Some(ch.channel_id);
        }
    }
    None
}

// Then assign spatial role based on nadir edge:
if let Some(gt51_ch) = detect_gt51_single_wing(&profiles) {
    if let Some(profile) = profiles.iter_mut().find(|p| p.channel_id == gt51_ch) {
        profile.spatial_role = match profile.nadir_edge {
            NadirEdge::Left  => SpatialRole::SingleSidePort,
            NadirEdge::Right => SpatialRole::SingleSideStarboard,
            _ => {
                // Default to port for GT51 (most common configuration)
                SpatialRole::SingleSidePort
            }
        };
    }
}
```

---

## 4. GT56 Channel Identification (Chirp vs Side Scan)

### 4.1 GT56 UHD2+ Channel Layouts

GT56 supports **three firmware-dependent layouts**:

| Layout | Sidescan Pair | Chirp Downscan | Depth/Temp | Observed In |
|--------|---------------|----------------|------------|-------------|
| **8-series** | ch8, ch9 | ch10 | ch11 | Standard UHD2 |
| **10-series** | ch10, ch11 | ch12 | ch13 | 25MAR25+ firmware |
| **14-series** | ch14, ch15 | ch16 | ch17 | Highest-end UHD2 |
| **ClearVü** | - | ch18, ch20 | - | Dual-freq ClearVü |

### 4.2 Current Disambiguation Logic

```rust
// garmin_rsd_parser.rs line ~791
// NOTE: ch10 is AMBIGUOUS — on older firmware it is chirp_downscan;
// on newer firmware it is port_sidescan.
// `find_sidescan_pair` resolves this at runtime by checking whether
// ch10 AND ch11 both have large sonar ping counts.
```

**Runtime detection in outputs.rs:**
```rust
// Check if ch10+ch11 are BOTH present with similar ping counts
// → 10-series sidescan pair
// Else if ch10 present but ch11 absent
// → 8-series chirp downscan
```

### 4.3 Issue: Chirp Channel Misidentified as Sidescan

**Evidence from test file `25MAR25-0736-01_2/`:**
- Channels present: 10, 11, 12, 13
- Expected: ch10=port, ch11=starboard, ch12=chirp, ch13=depth/temp
- Observed: **ch12 classified as SideVü** (wrong!)

**Root cause**: The archetype classifier uses nadir gap width as primary signal:

```rust
// channel_discovery.rs line ~900-950
fn classify_archetype(...) {
    // Signal 1: Nadir Gap Width
    if median_gap >= 20 {
        side_score += 3.0;  // ← High score for wide gap
    }
    
    // Signal 0: Channel ID Prior
    match ch_id {
        2 | 6 | 10 | 12 | 16 | 18 | 20 => {
            down_score += 3.0;  // ← Known downscan IDs
        }
        _ => {}
    }
}
```

**Problem**: ch12 in 10-series layout has wide nadir gap (similar to sidescan), overriding the channel ID prior.

### 4.4 Recommended Fix

**Strengthen channel ID prior for known chirp channels:**
```rust
fn classify_archetype(...) {
    // Signal 0: Channel ID Prior (EVALUATED FIRST, HIGHER WEIGHT)
    match ch_id {
        2 | 6 | 10 | 12 | 16 | 18 | 20 => {
            // Known downscan channels - STRONG PRIOR
            down_score += 5.0;  // ↑ Increased from 3.0
            reasons.push(format!("ch{}=known_downscan_id (strong)", ch_id));
        }
        993 | 1487 => {
            down_score += 2.5;
            reasons.push(format!("ch{}=legacy_downscan", ch_id));
        }
        7 | 11 | 13 | 17 => {
            // Depth/temp - disqualify from sidescan
            down_score += 4.0;
            reasons.push(format!("ch{}=depth_temp_id", ch_id));
        }
        _ => {}
    }
    
    // Only evaluate nadir gap if channel ID is ambiguous
    if down_score < 3.0 {
        // Signal 1: Nadir Gap Width (reduced weight)
        if median_gap >= 20 {
            side_score += 2.0;  // ↓ Reduced from 3.0
            // ...
        }
    }
}
```

**Add firmware layout detection:**
```rust
fn detect_firmware_layout(parsed: &ParseResult) -> &'static str {
    let channels: BTreeSet<u32> = parsed.pings.iter().map(|p| p.channel).collect();
    
    // 10-series: ch10+ch11 both present with similar counts
    if channels.contains(&10) && channels.contains(&11) {
        let ch10_count = parsed.channel_counts.get(&10).copied().unwrap_or(0);
        let ch11_count = parsed.channel_counts.get(&11).copied().unwrap_or(0);
        if (ch10_count as f64 / ch11_count as f64).abs() < 0.2 {
            return "10-series";
        }
    }
    
    // 14-series: ch14+ch15 present
    if channels.contains(&14) && channels.contains(&15) {
        return "14-series";
    }
    
    // 8-series: ch8+ch9 present
    if channels.contains(&8) && channels.contains(&9) {
        return "8-series";
    }
    
    "unknown"
}
```

---

## 5. Payload Structure Analysis

### 5.1 Record Structure Validation

All test files conform to the documented RSD format:

```
┌─────────────────────────────────────────┐
│ Header varstruct (15 fields)            │
│   - Magic: 0xB7E9DA86                   │
│   - Firmware: 0x0B02_0102 (v11.2.1.2)   │
│   - Sequence number                     │
│   - Transducer ID: 0xFFFFFFFF (N/A)     │
│   - data_size: u16                      │
│   - Timestamp: ms                       │
├─────────────────────────────────────────┤
│ Body varstruct                          │
│   - Channel ID: 1-4 bytes (MAY BE ABSENT)│
│   - Depth: zigzag varint (mm)           │
│   - Sample count: u32                   │
│   - GPS: lat/lon (Garmin units)         │
│   - Beam angle: f32                     │
│   - Beam type: u32 (1=classic, 2-4=UHD) │
├─────────────────────────────────────────┤
│ Sonar samples: u8 or i16                │
├─────────────────────────────────────────┤
│ Trailer (12 bytes)                      │
│   - Magic: 0xD9264B7C                   │
│   - chunk_size: u32                     │
│   - CRC: u32                            │
└─────────────────────────────────────────┘
```

### 5.2 Anomalies Detected

1. **Missing body field 0** (GT54 test capture)
   - Impact: All records → channel 0
   - Recovery: Use preamble metadata or beam type enum

2. **CRC mismatches** (all files, ~5-15% of records)
   - Impact: Advisory only, parsing continues
   - Root cause: Firmware writes during DMA flush

3. **Invalid GPS** (test captures without real position)
   - Values: 0°, ±180°, 0x80000000
   - Handling: Filtered out in GPS coverage metrics

4. **Sample count mismatch** (GT56 UHD2+)
   - Expected: `sonar_size == sample_count * bytes_per_sample`
   - Observed: ±8 byte tolerance
   - Risk: Ghost stripes in mosaic if misaligned

---

## 6. Recommendations Summary

### 6.1 Critical Fixes

1. **Fix GT54 body field 0 recovery**
   - Priority: HIGH
   - Impact: Restores port/starboard separation
   - Effort: 2-3 hours

2. **Remove redundant nadir flip test**
   - Priority: MEDIUM
   - Impact: Code clarity, prevents confusion
   - Effort: 30 minutes

3. **Strengthen chirp channel ID prior**
   - Priority: HIGH
   - Impact: Fixes GT56 ch12 misclassification
   - Effort: 1 hour

### 6.2 Enhancements

4. **Add GT51 single-wing detection**
   - Priority: MEDIUM
   - Impact: Better GT51 support
   - Effort: 2 hours

5. **Add firmware layout detection**
   - Priority: MEDIUM
   - Impact: Automatic 8/10/14-series disambiguation
   - Effort: 3 hours

6. **Add channel alignment persistence**
   - Priority: LOW
   - Impact: User overrides saved across sessions
   - Effort: 4 hours

### 6.3 Documentation Updates

7. **Update GARMIN_RSD_FORMAT.md**
   - Add body field 0 absence note
   - Document GT51 single-wing geometry
   - Clarify 8/10/14-series layouts
   - Add chirp channel ID table

---

## 7. Test Plan

### 7.1 Unit Tests

```rust
#[test]
fn gt54_missing_body_field_0_recovery() {
    // Verify channel recovery from preamble metadata
}

#[test]
fn gt51_single_wing_detection() {
    // Verify GT51 classified as SingleSide-Port/Starboard
}

#[test]
fn gt56_chirp_channel_classification() {
    // Verify ch12 classified as DownVü, not SideVü
}

#[test]
fn firmware_layout_detection() {
    // Verify 8/10/14-series auto-detection
}
```

### 7.2 Integration Tests

Run full pipeline on:
- `126SV-UHD2-GT54.RSD` → Verify port/starboard separation
- `Sonar011cvgt51.rsd` → Verify GT51 single-wing
- `25MAR25-0736-01_2/` → Verify ch10/11=port/star, ch12=chirp

---

## Appendix A: Channel ID Reference Table

| Channel ID | GT51 | GT54 | GT56 | Type | Generation | Sample Format |
|------------|------|------|------|------|------------|---------------|
| 0 | ✓ | - | - | port_sidescan | classic | u8 |
| 1 | ✓ | ✓* | - | starboard_sidescan | classic | u8 |
| 2 | ✓ | ✓* | - | chirp_downscan | classic | u8 |
| 3 | ✓ | - | - | port_sidescan | classic | u8 |
| 4 | - | ✓ | ✓ | port_sidescan | uhd | i16 |
| 5 | - | ✓ | ✓ | starboard_sidescan | uhd | i16 |
| 6 | - | ✓ | ✓ | chirp_downscan | uhd | i16 |
| 7 | - | ✓ | ✓ | depth_temp | uhd | metadata |
| 8 | - | ✓ | ✓ | port_sidescan | uhd2 | i16 |
| 9 | - | ✓ | ✓ | starboard_sidescan | uhd2 | i16 |
| 10 | - | ✓ | ✓ | chirp_downscan / port_sidescan* | uhd2 | i16 |
| 11 | - | ✓ | ✓ | depth_temp / starboard_sidescan* | uhd2 | metadata/i16 |
| 12 | - | - | ✓ | chirp_downscan | uhd2 | i16 |
| 13 | - | - | ✓ | depth_temp | uhd2 | metadata |
| 14 | - | - | ✓ | port_sidescan | uhd2+ | i16 |
| 15 | - | - | ✓ | starboard_sidescan | uhd2+ | i16 |
| 16 | - | - | ✓ | chirp_downscan | uhd2+ | i16 |
| 17 | - | - | ✓ | depth_temp | uhd2+ | metadata |
| 18 | - | ✓ | ✓ | chirp_downscan (ClearVü HF) | uhd2 | i16 |
| 20 | - | ✓ | ✓ | chirp_downscan (ClearVü) | uhd2 | i16 |
| 993 | - | - | ✓ | chirp_downscan (legacy) | uhd | i16 |
| 1487 | - | - | ✓ | chirp_downscan (Ultra) | uhd | i16 |

\* GT54 test capture `126SV-UHD2-GT54.RSD` has body field 0 absent → all records default to channel 0

---

*This analysis produced by automated test corpus scanning and reverse-engineering.*
