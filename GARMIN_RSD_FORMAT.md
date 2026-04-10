down scan can be called chirp# Garmin RSD Binary Format — Reverse Engineering White Paper

**Status:** Reverse-engineered from captured device data and Python parser archaeology
**Reference implementations:** `engine_nextgen_syncfirst.py`, `engine_classic_varstruct.py`, `core_shared.py` (Garmin_RSD_DualEngine_GUI_v5)
**Validated against:** Holloway.RSD, Sonar000.RSD, Sonar001.RSD, 126SV-UHD2-GT54.RSD, 126SV-UHD2-GT56.RSD, 25MAR25-0736-01_2/
**Date:** 2026-02-24 (updated 2026-03-31 with GT51/GT56 findings)

---

## 1. Overview

Garmin `.RSD` files are binary sonar capture logs produced by Garmin fishfinders (e.g. ECHOMAP Ultra, STRIKER, Panoptix). Each file is a contiguous stream of variable-length records. Each record contains a header, a body carrying navigation metadata, raw sonar sample bytes, and a trailer.

The format uses a self-describing length-coded encoding (called *varstruct* here) for both header and body. Sonar sample data is appended as a flat byte array directly after the body.

There is **no global file header** — the stream begins directly with the first record. Recovery after corruption is possible by scanning for the 4-byte record magic.

---

## 2. Magic Bytes

| Constant         | Value (LE hex) | Role |
|------------------|---------------|------|
| `MAGIC_REC_HDR`  | `86 DA E9 B7` | Identifies the start of every record header varstruct (stored as field 0 of the header) |
| `MAGIC_REC_TRL`  | `7C 4B 26 D9` | Identifies the trailing chunk descriptor |

The magic appears as field 0 **inside** the header varstruct, not as a bare prefix — so the actual bytes of the header start 1–64 bytes **before** the position where the magic bytes occur in the raw file (the varstruct field-count byte and the key byte for field 0 precede the magic value).

### Known magic variants
Some firmware builds encode variants: `0xB7E9DA87`, `0xB7E9DA88`, `0xB7E9DA89`. These are accepted by the parser's candidate list.

---

## 3. Varstruct Encoding

Both the record header and the record body use the same self-describing encoding:

```
[field_count: varuint]
[field_0_key: varuint] [field_0_value: bytes]
...
[field_N_key: varuint] [field_N_value: bytes]
[crc: u32 LE]
```

### 3.1 Varuint
Variable-length unsigned integer. Little-endian base-128:
- Each byte contributes 7 bits of value.
- Bit 7 (MSB) set = more bytes follow; cleared = last byte.
- Maximum decoded width: 35 bits (5 bytes).

### 3.2 Field Key
`key = (field_number << 3) | length_code`

| `length_code` | Meaning |
|---------------|---------|
| 0–6 | Value is exactly `length_code` bytes |
| 7 | Next varuint is an explicit byte count; value is that many bytes |

```python
fn_id = key >> 3
lc    = key & 7
vlen  = lc if lc != 7 else read_varuint()
value = read_bytes(vlen)
```

### 3.3 CRC
A 4-byte CRC follows the last field value. It is stored **little-endian** (`struct.unpack('<I', ...)`).

Algorithm (custom CRC-32):
```
poly = 0x04C11DB7
crc  = 0
for each byte b:
    crc ^= (b << 24)
    for 8 iterations:
        if crc & 0x80000000: crc = (crc << 1) ^ poly
        else: crc <<= 1
# Bit-reverse the 32-bit result, then XOR with 0xFFFFFFFF
```

> **Important:** CRC mismatches are common in captured files. Firmware writes records at high speed; the CRC may reflect a prior firmware version or a different seed. **Treat CRC as advisory only.** The parser always runs in `Warn` mode — mismatches are counted and reported in `ParseResult.crc_mismatch_count` but never abort parsing.

---

## 4. Record Layout

```
┌─────────────────────────────────────────────┐
│  Header varstruct                           │ ← Header fields (see §5)
│  … ends with CRC (4 bytes LE)               │
├─────────────────────────────────────────────┤
│  Body varstruct                             │ ← Body fields (see §6)
│  … ends with CRC (4 bytes LE)               │
├─────────────────────────────────────────────┤
│  Sonar sample bytes                         │ ← flat array, no framing
│  size = header.data_size − body_varstruct_size│
├─────────────────────────────────────────────┤
│  Trailer (12 bytes, all LE)                 │
│    magic:      u32 = 0xD9264B7C             │
│    chunk_size: u32 = bytes from record start│
│                      to end of trailer      │
│    crc:        u32 (advisory)               │
└─────────────────────────────────────────────┘
```

### Positioning
- `data_size` (header field 4, u16 LE) = total bytes from *body varstruct start* to *end of sonar data* (does **not** include trailer).
- `sonar_offset` = body varstruct start + body varstruct byte count
- `sonar_size`   = `data_size` − body varstruct byte count
- `trailer_pos`  = body varstruct start + `data_size`
- `next_record`  = header start + `chunk_size` (from trailer)

---

## 5. Header Fields

| Field # | Type | Description |
|---------|------|-------------|
| 0 | u32 LE | Record magic (`0xB7E9DA86` or variant) |
| 1 | blob | Firmware/software version block. Observed constant `0x0B02_0102` across all records in a file (interpreted as v11.2.1.2 or similar BCD encoding). |
| 2 | u32 LE | Sequence number (monotonically increasing per capture session) |
| 3 | u32 LE | **Transducer identifier (candidate).** Observed as `0xFFFFFFFF` in all test files. `0xFFFFFFFF` is the standard Garmin/NMEA 2000 sentinel for "not available / no override" — consistent with a nominal (factory-matched) transducer where no explicit ID handshake is required. Garmin units support mismatched transducer pairings (e.g. non-UHD transducer on a UHD unit), so a real transducer ID would be expected here when the connected transducer differs from the unit's default. **Confirmation requires a capture with an explicitly non-matching transducer connected.** |
| 4 | u16 LE | `data_size` — body + sonar byte count |
| 5 | u32 LE | Timestamp in milliseconds (device uptime) |

---

## 6. Body Fields

| Field # | Type | Description |
|---------|------|-------------|
| 0 | u32 LE **padded** | Channel ID (may be 1–4 bytes; pad with trailing zeros before decoding) |
| 1 | zigzag varint | Depth in **millimetres** (decode: `(u >> 1) ^ -(u & 1)`, then ÷ 1000.0 = metres) |
| 2 | unknown | Present in some files; varies widely (~3000 distinct values). Possibly a rolling measurement or secondary counter. Purpose TBD. |
| 3 | u32 LE **padded** | Constant `0` in all observed files. Reserved / unused sensor field (possible: water temperature raw ADC, always 0 when no temp sensor connected). |
| 6 | u32 LE **padded** | Constant `2` in all observed files including GT54 UHD2 (ch1, 2, 5, 10). **Not** a per-frequency indicator — the same value appears for both sidescan (455 kHz) and downscan (800 kHz) channels. Likely a protocol generation or product-family enum (Gen2 = 2). |
| 7 | u32 LE **padded** | **Sonar sample count** — number of samples in the attached sonar byte array for this ping. Used directly by the parser to drive `decode_samples()`. Values vary with the unit's configured depth/range: `2048` = maximum range; lower values appear when range is reduced (observed: 823–2048 in Holloway, 823/883/1037/2048 in GT54). |
| 8 | u32 LE **padded** | Constant `0` in all observed files. Likely **transducer depth offset** in millimetres (0 = transducer flush at waterline, no manual offset entered). |
| 9 | i32 LE | Latitude in Garmin map units (°= value × 360 / 2³²) |
| 10 | i32 LE | Longitude in Garmin map units (°= value × 360 / 2³²) |
| 11 | f32 LE | Beam angle in degrees |
| 12 | u32 LE **padded** | **Beam-type enum** — identifies the transducer beam attached to this channel. Observed values: `1` (classic/gen1 starboard), `2` (UHD port / gen2 ch4), `3` (UHD starboard / gen2 ch5), `4` (additional UHD2 beam). In GT54: `{1:1834, 2:2434, 3:2434, 4:2434}` records respectively. In Holloway (ch4/5 only): `{2, 3}`. Correlates with channel ID but is an independent hardware descriptor. |
| 13 | u32 LE | **Device/unit identifier** — constant `150995206` (`0x0900_0006`) across all records in all observed files from the same hardware family. |
| 15 | u32 LE | **Format tag** — constant `1028` in all observed files. Likely a payload/format version used by the chartplotter's rendering pipeline. |

> **Note on gen1 field numbering:** The first-generation RSD format documented in the Memotech reference paper (see §14) uses different field indices. In particular, that paper assigns "Transducer XID / Operating State" semantics (TVG slope, gain coefficients, pulse width index) to what it labels "Field 7". In the gen2 format reverse-engineered here, body field 7 is the **sample count** — confirmed by parser correctness on multiple files. The XID/TVG metadata either appears under different field indices in gen2, or is folded into the header varstruct.

### 6.1 Padded u32 decode
Python: `int.from_bytes(val[:4].ljust(4, b'\x00'), 'little')`
Rust: copy bytes into a 4-byte zero-padded buffer, then `u32::from_le_bytes`.

This is **critical** — channel IDs 4 and 5 are stored as **single bytes** (value `04` / `05`). A strict 4-byte decode returns `None` and silently defaults to channel 0, losing port/starboard separation entirely.

### 6.1.1 Body Field 0 Absence (GT54 Anomaly)

**Critical finding:** Some GT54 UHD2 test captures (e.g., `126SV-UHD2-GT54.RSD`) have **body field 0 completely absent** in the varstruct. This causes all records to default to channel 0, losing port/starboard separation.

**Recovery strategies:**
1. **Preamble metadata block**: Scan for VS#0 f06 metadata block at file start, which encodes channel IDs in one of two formats:
   - NEW: `[03 03 01 01 CH ...]` → CH is the channel ID (1 byte, IDs 0-127)
   - OLD: `[03 04 01 02 LO HI …]` → LE-u16 [LO,HI] is the channel ID
2. **Beam type enum (field 12) + generation enum (field 6)**: Use hardware descriptors to infer channel role
3. **Filename heuristic**: Last resort — extract transducer type from filename (e.g., "GT54", "GT56")

**Parser implementation:**
```rust
let channel = le_u32_padded(body.get(&0).map(Vec::as_slice).unwrap_or(&[])).unwrap_or(0);
// ↑ When field 0 absent, defaults to 0 — recovery needed
```

**Affected files:**
- `126SV-UHD2-GT54.RSD` — all records → channel 0 (should be 1, 2, 5, 10)
- Similar test captures without GPS data

---

### 6.1.2 Complete Channel ID Reference Table

| ID | GT51 | GT54 | GT56 | Type | Generation | Sample Format | Notes |
|----|------|------|------|------|------------|---------------|-------|
| 0 | ✓ | - | - | port_sidescan | classic | u8 | GT51 asymmetric |
| 1 | ✓ | ✓* | - | starboard_sidescan | classic | u8 | GT54 test capture anomaly |
| 2 | ✓ | ✓* | - | chirp_downscan | classic | u8 | Also called "down scan" |
| 3 | ✓ | - | - | port_sidescan | classic | u8 | GT51 asymmetric |
| 4 | - | ✓ | ✓ | port_sidescan | uhd | i16 | Standard UHD sidescan |
| 5 | - | ✓ | ✓ | starboard_sidescan | uhd | i16 | Standard UHD sidescan |
| 6 | - | ✓ | ✓ | chirp_downscan | uhd | i16 | Also called "down scan" |
| 7 | - | ✓ | ✓ | depth_temp | uhd | metadata | No sonar samples |
| 8 | - | ✓ | ✓ | port_sidescan | uhd2 | i16 | 8-series layout |
| 9 | - | ✓ | ✓ | starboard_sidescan | uhd2 | i16 | 8-series layout |
| 10 | - | ✓ | ✓ | chirp_downscan / port_sidescan* | uhd2 | i16 | **AMBIGUOUS**: 8-series=chirp, 10-series=port |
| 11 | - | ✓ | ✓ | depth_temp / starboard_sidescan* | uhd2 | metadata/i16 | **AMBIGUOUS**: 8-series=depth, 10-series=star |
| 12 | - | - | ✓ | chirp_downscan | uhd2 | i16 | 10-series downscan |
| 13 | - | - | ✓ | depth_temp | uhd2 | metadata | 10-series |
| 14 | - | - | ✓ | port_sidescan | uhd2+ | i16 | 14-series layout |
| 15 | - | - | ✓ | starboard_sidescan | uhd2+ | i16 | 14-series layout |
| 16 | - | - | ✓ | chirp_downscan | uhd2+ | i16 | 14-series downscan |
| 17 | - | - | ✓ | depth_temp | uhd2+ | metadata | 14-series |
| 18 | - | ✓ | ✓ | chirp_downscan | uhd2 | i16 | ClearVü HF |
| 20 | - | ✓ | ✓ | chirp_downscan | uhd2 | i16 | ClearVü |
| 993 | - | - | ✓ | chirp_downscan | uhd | i16 | Legacy 93SV platform |
| 1487 | - | - | ✓ | chirp_downscan | uhd | i16 | ECHOMAP Ultra |

\* GT54 test capture `126SV-UHD2-GT54.RSD` has body field 0 absent → all records default to channel 0

**Firmware-dependent layouts (GT56 UHD2+):**
- **8-series**: ch8/9=port/star, ch10=chirp, ch11=depth
- **10-series** (25MAR25+ firmware): ch10/11=port/star, ch12=chirp, ch13=depth
- **14-series**: ch14/15=port/star, ch16=chirp, ch17=depth

**Runtime disambiguation required for ch10/ch11:**
```rust
// If BOTH ch10 AND ch11 have similar ping counts → 10-series sidescan pair
// If ONLY ch10 present with high count → 8-series chirp downscan
```

### 6.2 GPS coordinate encoding
Standard Garmin map unit: divide the 32-bit signed integer by `2³²` and multiply by `360°`.

```
degrees = i32_value * (360.0 / 4_294_967_296.0)
```

Invalid/no-fix readings appear as `0`, `±180.0°`, or the raw integer `0x80000000` (−180.0°).

### 6.3 Depth encoding
Depth is a **zigzag-encoded varint**. This allows small negative values (above transducer) to be stored compactly.

```
zigzag_unsigned = read_varuint(field_1_bytes)
depth_mm        = (zigzag_unsigned >> 1) ^ -(zigzag_unsigned & 1)
depth_m         = depth_mm / 1000.0
```

---

b

## 9. Recovery / Sync Strategy

Because there is no global header and CRCs may be wrong, the parser uses a scan-and-backtrack strategy:

1. **Find magic:** Scan forward for the 4-byte magic `86 DA E9 B7` (little-endian) in the raw bytes.
2. **Backtrack:** Walk backward from the magic position up to 64 bytes, attempting to parse a varstruct at each candidate start. The first one that successfully decodes with field 0 == magic is the record header start.
3. **Parse body:** Parse the varstruct immediately following the header.
4. **Trailer hop:** If a valid trailer is found at `body_start + data_size`, set `next_pos = header_start + chunk_size`. This skips directly to the next record.
5. **Fallback:** If the trailer is missing or invalid, scan forward for the next magic occurrence.
6. **Dropped bytes:** All bytes between the last known good record end and the next header start are counted as `dropped_bytes` and contribute to `recovered_records`.

---

## 10. File-Level Observations

| File | Device | Records | Ch4 | Ch5 | GPS Region | Notes |
|------|--------|---------|-----|-----|------------|-------|
| `Holloway.RSD` | ECHOMAP Ultra (Gen2) | ~81,390 | ~40,695 | ~40,695 | 43.1°N 83.4°W (Michigan) | Body field 0 present; single-byte 0x04/0x05 |
| `Sonar000.RSD` | ECHOMAP Ultra (Gen2) | ~170,327 | ~85,163 | ~85,162 | 44.0°N 83.4°W (Michigan) | Same encoding |
| `Sonar001.RSD` | ECHOMAP Ultra (Gen2) | ~15,466 | TBD | TBD | Michigan area | |
| `126SV-UHD2-GT54.RSD` | GT54 UHD2 (test unit) | ~9,136 | includes ch1+ch5 | ch2+ch10 | −180.0° (invalid) | Body field 0 present; single-byte values 1, 2, 5, 10. GPS is invalid — test capture without real position. Channel 10 type TBD. |
| `93SV-UHD-GT56_nextgen` | GT56 UHD | 173 | 0 | 0 | −180.0° (invalid) | Channel ID 993 in field 0 |

---

## 11. Known Firmware Variations

- **Channel 10 (and other high IDs):** Observed in GT54 captures (values 1, 2, 5, 10 in field 0). Channels 0–9 are mapped in the parser; anything above is surfaced as `unknown_channels`. Add entries to `map_channel_type()` as new variants are identified.
- **Magic variants +1/+2/+3:** Some firmware builds add 1–3 to the base magic. Handled via candidate list.
- **CRC algorithm variants:** Non-zero percentage of records fail CRC even on unmodified captures. Root cause unknown (possible: firmware writes records during DMA flush before CRC is finalized). Always parse in `Warn` mode.
- **data_size = 0:** Seen on first record of some files. Parser skips sonar extraction for this record.
- **Varstruct field_count = 0:** Valid (empty body). Parser handles gracefully.

---

## 11.1 Transducer-Specific Geometry Patterns

### GT51 (Classic/Asymmetric Single-Wing)

**Architecture:**
- Single asymmetric wing (NOT paired sidescan like GT54/GT56)
- Water column at ONE EDGE of sample array (not center)
- Two operating modes:
  - **Classic mode**: Channels 0-3 (8-bit u8 samples)
  - **ClearVü mode**: Channels 4, 6 (16-bit i16 samples)

**Nadir position:**
- **GT51 Port configuration**: Nadir at LEFT edge (index 0)
- **GT51 Starboard configuration**: Nadir at RIGHT edge (index max)
- **NOT at center** (unlike GT54/GT56 paired sidescan)

**Detection signature:**
```rust
// GT51 single-wing detection
if sidevu_channels.len() == 1 {
    let ch = sidevu_channels[0];
    if ch.channel_id <= 3 || (ch.channel_id == 4 || ch.channel_id == 6) {
        // Single SideVü channel with classic/UHD ID → GT51
        // Assign spatial role based on nadir edge:
        // - NadirEdge::Left  → SingleSidePort
        // - NadirEdge::Right → SingleSideStarboard
    }
}
```

**Sample characteristics:**
- Classic mode: 256-512 samples (8-bit u8)
- ClearVü mode: 800-2048 samples (16-bit i16)
- No paired sidescan channel (only ONE SideVü channel present)

### GT54 (UHD Standard Paired Sidescan)

**Architecture:**
- Full paired sidescan (port + starboard)
- Water column at CENTER (nadir gap between port/starboard arms)
- Standard channel layout: ch4=port, ch5=starboard, ch6=chirp

**Nadir position:**
- **After parser flip correction**: Nadir at LEFT edge of each channel's array
- **Before correction**: May appear at RIGHT edge (firmware-dependent)
- **Paired geometry**: Both channels have similar nadir gap width (30-200 samples)

**Detection signature:**
```rust
// GT54 paired sidescan detection
if sidevu_channels.len() >= 2 {
    // Score pairs by:
    // 1. Nadir gap similarity (paired arms see same water column)
    // 2. Sample count similarity (same transducer element)
    // 3. Ping count balance (both arms fire at same rate)
    // 4. Temporal overlap (both active simultaneously)
    // 5. GPS coverage (both have valid GPS)
}
```

### GT56 (UHD2+ Multi-Frequency)

**Architecture:**
- Multi-frequency sidescan (UHD + UHD2 layers)
- Firmware-dependent channel numbering (8/10/14-series layouts)
- ClearVü dual-frequency support (ch18=HF, ch20=standard)

**Firmware layouts:**

| Layout | Sidescan Pair | Chirp Downscan | Depth/Temp | Disambiguation |
|--------|---------------|----------------|------------|----------------|
| 8-series | ch8, ch9 | ch10 | ch11 | ch8+ch9 both present |
| 10-series | ch10, ch11 | ch12 | ch13 | ch10+ch11 both present with similar counts |
| 14-series | ch14, ch15 | ch16 | ch17 | ch14+ch15 both present |

**Chirp channel identification:**

Known chirp/downscan channel IDs: **2, 6, 10*, 12, 16, 18, 20, 993, 1487**

\* ch10 is AMBIGUOUS — treat as chirp unless ch11 present with similar count

**Detection algorithm:**
```rust
fn classify_archetype(...) {
    // Signal 0: Channel ID Prior (STRONGEST SIGNAL)
    match ch_id {
        2 | 6 | 10 | 12 | 16 | 18 | 20 => {
            down_score += 5.0;  // Strong prior for known chirp IDs
        }
        _ => {}
    }
    
    // Signal 1: Nadir Gap Width (SECONDARY, lower weight)
    if median_gap >= 20 {
        side_score += 2.0;  // Reduced weight to avoid chirp misclassification
    }
    
    // Signal 2: Sample Array Length
    if median_sample_count >= 800 {
        side_score += 1.0;  // Long swath → likely sidescan
    }
    
    // Signal 3: Early Peak (Bottom Spike)
    if spike.early_peak_ratio > 0.6 {
        down_score += 2.5;  // Strong downscan signal
    }
}
```

**Common misclassification issue:**

ch12 in 10-series layout is sometimes misclassified as SideVü because:
1. Wide nadir gap (similar to sidescan geometry)
2. Long sample array (800-2048 samples)

**Fix:** Strengthen channel ID prior to override nadir gap signal for known chirp IDs.

---

## 12. Implementation Notes

### Rust parser (`garmin_rsd_parser.rs`)
- `le_u32_padded()` — pads short byte slices with zeros before u32 decode (required for Gen2 channel IDs).
- `CrcMode::Warn` — ignores CRC mismatches, increments `ParseResult.crc_mismatch_count`.
- `CrcMode::Strict` — returns `Err(())`, used only in tests.
- `find_next_magic()` — scans byte-by-byte for any candidate magic value (handles +1/+2/+3 variants).
- `load_magic_candidates()` — reads optional `garmin_magic_variants.txt` from same directory as input file for user-extensible magic list.

### Python reference (v5 DualEngine)
- `core_shared._parse_varstruct()` — reference implementation; used to validate Rust output.
- `engine_nextgen_syncfirst` — `CrcMode::Warn` equivalent; handles mismatches with `logging.warning`.
- `engine_classic_varstruct` — `CrcMode::Strict`; will refuse corrupt records.
- `engine_glue.run_engine("both")` — runs both engines and picks the higher-scoring result.

---

## 13. Hex Example (Holloway.RSD, first valid record at offset 0x5031)

```
Offset   Bytes (hex)                                   Annotation
------   ───────────────────────────────────────       ──────────
5031     0F                                            varstruct field_count = 15 (varuint)
5032     02                                            key = 0x02 → fn=0 lc=2 → field 0, 2 bytes???
         [field 0 = channel_id, 1 byte value = 04]    channel = 4 (port_sidescan)
...      [field 2 = seq]
...      [field 5*8 = 40 = timestamp key]
...      [field 9 = lat mapunits → 43.125°N]
...      [field 10 = lon mapunits → −83.434°W]
...      [CRC 4 bytes LE]                              body varstruct CRC (may mismatch)
         [sonar bytes × sample_count]                  raw backscatter
         [7C 4B 26 D9]                                 MAGIC_REC_TRL
         [chunk_size u32 LE]                           → next record = 0x5031 + chunk_size
         [trailer CRC u32 LE]
```

---

---

## 14. Reference Materials

| Source | Applicability | Notes |
|--------|--------------|-------|
| [Garmin RSD Format (Memotech/Franken)](https://www.memotech.franken.de/FileFormats/Garmin_RSD_Format.pdf) | **Gen1 only** | Documents the classic/first-generation RSD binary format. Field numbers and semantic assignments in that paper do **not** map directly to the gen2 format documented here. Useful for understanding the lineage of the varstruct encoding and magic byte scheme, but treat individual field index↔meaning mappings as gen1-specific. |
| `Garmin_RSD_DualEngine_GUI_v5` (Python) | Gen1 + Gen2 | Reference implementation used during reverse engineering. `engine_nextgen_syncfirst.py` targets gen2; `engine_classic_varstruct.py` targets gen1. |
| Captured `.RSD` files | Both | Primary evidence for all field assignments in §5 and §6. Field entries without explicit provenance are inferred from low-cardinality value analysis across `Holloway.RSD` and `126SV-UHD2-GT54.RSD`. |

### Gen1 vs Gen2 field-index divergence

The gen1 paper describes a "Field 7" carrying Transducer Identification (XID) data including TVG slope, master gain coefficient, and pulse-width index. The values cited (823 ≈ 800 kHz frequency index, 1037 ≈ reference gain, 883 ≈ TVG slope, 2048 ≈ buffer size) are plausible for gen1 XID semantics.

In gen2, body **field 7 is the sample count** — the parser reads it at line 271 of `garmin_rsd_parser.rs` and passes it directly to `decode_samples()`. The same numeric range (823–2048) appears in gen2 field 7 because sample counts happen to fall in the same range as gen1 XID values (both are O(10²–10³) integers). The semantic is entirely different:

| Value | Gen1 interpretation (XID) | Gen2 interpretation (sample count) |
|-------|--------------------------|-------------------------------------|
| 2048 | Buffer/FFT bin count | Max-range sample count (standard) |
| 1037 | Raw sensitivity / master gain | Reduced sample count (mid range) |
| 883 | TVG slope coefficient | Reduced sample count (short range) |
| 823 | Frequency index (800 kHz) | Minimum observed sample count |

The gen2 XID/TVG data location is currently unknown — it may be in the header varstruct, in a file-level metadata record, or packed into body fields not yet decoded.

---

*This document was produced by reverse-engineering captured binary data. No official Garmin documentation was used. Field assignments marked TBD should be treated as tentative.*
