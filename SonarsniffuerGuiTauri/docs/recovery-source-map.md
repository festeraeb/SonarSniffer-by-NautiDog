# Crash Recovery Source Map

This file maps where mixed project logic was recovered from so the Rust port can continue cleanly.

## Canonical Parser Sources (Python)

- `D:/Temp/sonarsniffer_tauri_prototype/sonarsniffer_repo/src/sonarsniffer/core_shared.py`
- `D:/Temp/sonarsniffer_tauri_prototype/sonarsniffer_repo/src/sonarsniffer/engine_nextgen_syncfirst.py`
- `D:/Temp/sonarsniffer_tauri_prototype/sonarsniffer_repo/src/sonarsniffer/adapters/rsd_adapter.py`

Key logic recovered:
- Header magic candidates and sync search.
- Varstruct decoding with custom CRC behavior.
- Garmin mapunit-to-degree coordinate conversion.
- RSD record body fields: channel, sample count, lat/lon, depth varint, beam angle.
- Sonar payload format heuristics (`u8` vs `int16`) used for adapters.

## Rust Recovery Sources

- `C:/Users/thomf/programming/sonarsnifferrust/rust-garmin-rsd-cli/src/firmware_lookup.rs`
- `C:/Users/thomf/programming/sonarsnifferrust/rust-garmin-rsd-cli/src/firmware_strings.rs`

Key logic recovered:
- Firmware float identifier extraction.
- XOR block extraction and ASCII block heuristics.

## Sample Corpus

Primary sample corpus:
- `D:/Temp/cesarops_repo_tmp/Garminjunk/archive/HistoryofCESARSNIFFERBAGFILE/Sonar Samples`

Notable files:
- Garmin `.RSD`: multiple UHD/UHD2 captures.
- Humminbird `.DAT`: `R00003.DAT`, `R00004.DAT`, `R00012.DAT`.
- Existing output references: CSV/JSON/KML/HTML files in the same folder.

## Project Separation Notes

- `cesarops_repo_tmp` remains the parent SAR/ops pipeline target.
- `SonarsniffuerGuiTauri` is the standalone commercial tool path.
- Recovered parser and firmware modules were ported into Tauri backend first, so they can later be invoked by CESAROPS integration.
