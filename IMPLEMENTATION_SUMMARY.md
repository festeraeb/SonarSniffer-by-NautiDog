# Sonar Channel Detection Fixes - Implementation Summary

**Date:** 2026-03-31  
**Status:** ✅ All critical fixes implemented

---

## Overview

This document summarizes the code changes implemented to fix channel detection and orientation issues in the sonarsniffer_core parser, based on the analysis in `TEST_FILES_ANALYSIS.md`.

---

## Changes Implemented

### 1. Body Field 0 Recovery (garmin_rsd_parser.rs)

**Problem:** GT54 test capture `126SV-UHD2-GT54.RSD` has body field 0 (channel ID) absent, causing all records to default to channel 0 and losing port/starboard separation.

**Solution:** Added recovery logic that uses:
- Beam type enum (body field 12)
- Generation enum (body field 6)  
- Preamble channel IDs (VS#0 f06 metadata block)

**Code changes:**
```rust
// Line ~470-510 in garmin_rsd_parser.rs
let channel_from_body = le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[]));

let channel = if let Some(ch) = channel_from_body {
    ch
} else {
    // Recovery from beam type + generation enum
    let beam_type = le_u32_padded(body.get(&12)...).unwrap_or(0);
    let gen_enum = le_u32_padded(body.get(&6)...).unwrap_or(0);
    
    match (beam_type, gen_enum) {
        (1, _) => 1,  // classic starboard
        (2, 2) => 4,  // UHD port
        (3, 2) => 5,  // UHD starboard
        // ... UHD2 recovery using preamble channels
    }
};
```

**Additional changes:**
- Added `preamble_channels` parameter to `try_parse_record()` function
- Preamble channels extracted once at start of parsing loop
- Recovery logged to stderr for debugging

---

### 2. Strengthened Chirp Channel ID Prior (channel_discovery.rs)

**Problem:** GT56 ch12 in 10-series layout misclassified as SideVü due to wide nadir gap, even though it's a known chirp downscan channel.

**Solution:** 
- Increased channel ID prior weight from 3.0 → 5.0 for known chirp channels
- Reduced nadir gap weight from 3.0 → 2.0 to prevent override
- Channel ID is now the STRONGEST signal

**Code changes:**
```rust
// Line ~905-920 in channel_discovery.rs
match ch_id {
    2 | 6 | 10 | 12 | 16 | 18 | 20 => {
        down_score += 5.0;  // ↑ from 3.0
        reasons.push(format!("ch{}=known_downscan_id (strong prior)", ch_id));
    }
    993 | 1487 => {
        down_score += 3.5;  // ↑ from 2.5
        reasons.push(format!("ch{}=legacy_downscan", ch_id));
    }
    // ...
}

// Nadir gap weight reduced
if median_gap >= 20 {
    side_score += 2.0;  // ↓ from 3.0
    // ...
}
```

---

### 3. GT51 Single-Wing Detection Enhancement (channel_discovery.rs)

**Problem:** GT51 asymmetric transducers need special handling for proper port/starboard assignment.

**Solution:** Enhanced detection using channel ID signatures:
- Classic mode: channels 0-3
- ClearVü mode: channels 4, 6
- Nadir edge determines port vs starboard

**Code changes:**
```rust
// Line ~295-340 in channel_discovery.rs
for p in profiles.iter_mut() {
    if p.archetype == SignalArchetype::SideVu && p.spatial_role == SpatialRole::Unassigned {
        let is_gt51_classic = p.channel_id <= 3;
        let is_gt51_clearvu = p.channel_id == 4 || p.channel_id == 6;
        
        if is_gt51_classic || is_gt51_clearvu {
            p.spatial_role = match p.nadir_edge {
                NadirEdge::Left  => SpatialRole::SingleSidePort,
                NadirEdge::Right => SpatialRole::SingleSideStarboard,
                _ => SpatialRole::SingleSidePort, // default
            };
            log.push(format!(
                "ch{}: GT51 {} + nadir={:?} → {:?} (single-wing asymmetric)",
                p.channel_id,
                if is_gt51_classic { "classic" } else { "ClearVü" },
                p.nadir_edge,
                p.spatial_role
            ));
        }
    }
}
```

---

### 4. Firmware Layout Detection (channel_discovery.rs)

**Problem:** GT56 UHD2+ has three firmware-dependent layouts (8/10/14-series) with ambiguous ch10/ch11 channel roles.

**Solution:** Added `detect_firmware_layout()` function that analyzes channel presence and ping count balance to determine layout:

**Code changes:**
```rust
// Line ~218-310 in channel_discovery.rs
pub enum FirmwareLayout {
    Series8,     // ch8/9=port/star, ch10=chirp
    Series10,    // ch10/11=port/star, ch12=chirp (25MAR25+)
    Series14,    // ch14/15=port/star, ch16=chirp
    UhdClassic,  // ch4/5=port/star, ch6=chirp
    Unknown,
}

pub fn detect_firmware_layout(profiles: &[ChannelProfile]) -> FirmwareLayout {
    // 10-series: ch10+ch11 BOTH present with similar counts
    if channel_ids.contains(&10) && channel_ids.contains(&11) {
        if ch10_count > 10 && ch11_count > 10 && counts_balanced {
            return FirmwareLayout::Series10;
        }
    }
    // 8-series, 14-series, UHD classic detection...
}
```

**Integration:** Layout detection runs after profiling and can override misclassified channels in 10-series mode.

---

### 5. Removed Redundant nadir_flip_test() (channel_discovery.rs)

**Problem:** `nadir_flip_test()` function was redundant with `ParseResult::normalize_nadir_direction()` which already handles sample flipping at parse time.

**Solution:** 
- Removed the entire `nadir_flip_test()` function (~30 lines)
- Removed the call site in the discovery pipeline
- Added comment explaining that nadir flip is handled by parser

**Code changes:**
- Deleted function at line ~1195-1220
- Updated step 2a to note parser handles it
- Renumbered subsequent sections

---

## Documentation Updates

### GARMIN_RSD_FORMAT.md

**Added sections:**
- **6.1.1 Body Field 0 Absence (GT54 Anomaly)** - Documents the recovery strategies
- **6.1.2 Complete Channel ID Reference Table** - All 28 channel IDs with transducer support matrix
- **11.1 Transducer-Specific Geometry Patterns** - GT51/GT54/GT56 architecture differences
  - GT51 asymmetric single-wing geometry
  - GT54 paired sidescan geometry
  - GT56 multi-frequency layouts
  - Chirp channel identification algorithm

### TEST_FILES_ANALYSIS.md (New File)

**Comprehensive analysis document with:**
- Test files catalog (929 files analyzed)
- Hidden/embedded data discoveries
- Root cause analysis for each issue
- Recommended fixes with code examples
- Test plan for validation

---

## Files Modified

| File | Lines Changed | Description |
|------|---------------|-------------|
| `src-tauri/src/garmin_rsd_parser.rs` | ~50 | Body field 0 recovery, preamble channel passing |
| `src-tauri/src/channel_discovery.rs` | ~200 | Chirp prior, GT51 detection, firmware layout, cleanup |
| `GARMIN_RSD_FORMAT.md` | ~150 | New sections documenting findings |
| `TEST_FILES_ANALYSIS.md` | ~400 | New comprehensive analysis document |

---

## Testing Recommendations

### Unit Tests to Add

```rust
#[test]
fn gt54_body_field_0_recovery() {
    // Verify channel recovery from beam type enum
}

#[test]
fn gt56_chirp_channel_classification() {
    // Verify ch12 classified as DownVü in 10-series
}

#[test]
fn gt51_single_wing_detection() {
    // Verify GT51 classified as SingleSide-Port/Starboard
}

#[test]
fn firmware_layout_detection() {
    // Verify 8/10/14-series auto-detection
}
```

### Integration Tests

Run full pipeline on:
- `126SV-UHD2-GT54.RSD` → Verify port/starboard separation restored
- `Sonar011cvgt51.rsd` → Verify GT51 single-wing detection
- `25MAR25-0736-01_2/` → Verify ch10/11=port/star, ch12=chirp

---

## Expected Impact

### Before Fixes
- GT54 test file: All records → channel 0 (no port/star separation)
- GT56 10-series: ch12 misclassified as SideVü
- GT51: May be unassigned or misclassified
- Firmware layout: Manual intervention required

### After Fixes
- GT54 test file: Channels recovered from beam type + preamble
- GT56 10-series: ch12 correctly classified as DownVü/Chirp
- GT51: Properly detected and assigned as SingleSide-Port/Starboard
- Firmware layout: Automatic detection and disambiguation

---

## Remaining Work (Optional Enhancements)

1. **Channel alignment persistence** - Save user overrides across sessions
2. **Enhanced preamble recovery** - Scan for VS#0 f06 in more detail
3. **Multi-frequency sidescan support** - Handle UHD + UHD2 layers simultaneously
4. **Unit test suite** - Add comprehensive tests for all new functions

---

## Build Verification

Run the following to verify the changes compile:

```bash
cd src-tauri
cargo build --release
```

Expected output: No compilation errors, warnings acceptable for new code.

---

*Implementation completed by Qwen Code on 2026-03-31*
