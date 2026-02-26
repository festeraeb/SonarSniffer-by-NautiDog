# SonarSniffer GUI (Tauri + Rust)

Rust-first sonar processing pipeline with a lightweight Tauri UI.

Current focus:
- Garmin-first parsing with smart self-healing resync.
- Fast output generation: waterfall, mosaic, MBTiles, KML/KMZ, ArcGIS sidecar.
- Native-free viewer bundle (MapLibre-based static viewer).
- Feature-gated GStreamer integration path for video exports.

## What Is Implemented

Backend (`src-tauri/src`):
- `garmin_rsd_parser.rs`: resilient Garmin parser core with sync recovery.
- `outputs.rs`: artifact pipeline for:
	- `waterfall.png`
	- `mosaic.png`
	- `sonar.mbtiles`
	- `track.kml`
	- `track.kmz`
	- `arcgis_layer.json`
	- `viewer/` (native-free static map viewer)
- `video.rs`: GStreamer video export hook via Cargo feature `video-gstreamer`.
- `lib.rs`: Tauri commands:
	- `pick_input_file`
	- `run_sonar_pipeline`

Frontend (`src`):
- File picker invocation from Rust command (stable absolute-path selection).
- Advanced export options passed to Rust pipeline.
- JSON pipeline summary view.

## Build / Run

```powershell
cd src-tauri
cargo check
```

Tauri dev flow from workspace root:

```powershell
npm install
npm run tauri dev
```

## Optional GStreamer Enablement

Video export is intentionally feature-gated to avoid breaking standard developer setups.

```powershell
cd src-tauri
cargo check --features video-gstreamer
```

Install local GStreamer runtime/dev packages first, then wire pipeline details in `src-tauri/src/video.rs`.

## Performance / Quality Direction

- Keep heavy processing in Rust.
- Avoid unnecessary copying and format churn.
- Prefer streaming/chunked decode for very large logs.
- Move expensive mosaic blending and georeferencing into dedicated worker tasks.

## Planned Next Phases

1. Add parser trait layer and register additional vendor decoders in Rust.
2. Integrate firmware-derived field maps/signatures into parser recovery heuristics.
3. Improve mosaic georeferencing and overlap blending to rival commercial tooling.
4. Add ENC/NOAA vector chart overlay integration in bundled viewer.
5. Add benchmark suite and golden-output regression tests.

## Repository Context Notes

- At the time of this implementation pass, the repo contains a single markdown file (`README.md`) and no firmware/deconstruction files under this workspace path.
- If firmware artifacts live in another folder, add or link them here and they can be folded into the Rust parser framework next.
