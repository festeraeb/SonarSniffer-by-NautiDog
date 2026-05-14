gstreamer encoder prototype

This directory contains a small Rust-based gstreamer encoder binary (`tools/gstreamer_encoder`) that reads raw RGB frames from stdin and writes an MP4 using the best available encoder element on the host.

Build & run
-----------
- Requires GStreamer development libraries and plugins installed for desired encoders. On Linux you will also need `pkg-config` and the `*-dev` packages for GStreamer (e.g., `libgstreamer1.0-dev`, `libgstreamer-plugins-base1.0-dev`).
- Build with `cargo build --release` in `tools/gstreamer_encoder` (or `cargo build` for debug). Use `scripts/build_gst_encoder.sh` (Unix) or `scripts/build_gst_encoder.ps1` (Windows) to build easily.
- Example usage (assumes binary at tools/gstreamer_encoder/target/release/gst_encoder):
  gst_encoder --width=2048 --height=64 --fps=15 --output=out.mp4
- Image-sequence mode: you can pass a directory of frames (PNG/JPG) directly to the encoder which is useful for disk-based workflows and fast non-streaming in-field encodes:
  gst_encoder --input-dir=/path/to/frames --width=2048 --height=64 --fps=15 --output=out.mp4
  The Python bridge will prefer this mode as a fallback if streaming fails, avoiding a separate ffmpeg invocation.
- CI artifacts & telemetry validation: CI uploads built `gst_encoder` binaries as artifacts for each OS. If you set `AZURE_TEST_STORAGE_CONNSTR` and `AZURE_TEST_STORAGE_CONTAINER` in your repo secrets, CI will also run a telemetry validation job that uploads a small test JSON to the configured Azure storage container.- For CI: the repository includes a GitHub Actions workflow `.github/workflows/gstreamer_encoder.yml` that builds the binary and runs `scripts/test_gstreamer_encoder.py` forcing `x264enc` for reliable test coverage on Ubuntu runners.
Integration with Python
-----------------------
- Set `VIDEO_ENCODER=gstreamer` to use the gstreamer bridge for `scans_to_video` and other export paths.
- Optionally set `GST_ENCODER_PATH` to point at the compiled binary.
- Telemetry: the bridge reports runtime errors and fallback events via `sonarsniffer.telemetry.report_runtime_error` when it has issues; configure `SONARSNIFFER_TELEMETRY_URL` and `SONARSNIFFER_TELEMETRY_TOKEN` (see `docs/azure_telemetry.md`) to send reports to your Azure receiver.

Notes & limitations
-------------------
- The encoder will attempt to select the best encoder element using `gst_element_factory_find`-style checks. For h264/h265 encoders the code will automatically insert the appropriate parser (`h264parse` / `h265parse`) before `mp4mux` to ensure proper linking. If a hardware encoder fails to create a working pipeline (common when GPU plugins aren't available or require special contexts), the bridge will try a gst-launch pipeline that explicitly inserts the parser; if that also fails it will fall back to software `x264enc`, and as a final fallback it will write PNG frames to a temporary directory and invoke `ffmpeg` to encode the sequence. This layered fallback strategy provides robustness for field runs.
- The pipeline includes `videoconvert` so input RGB frames are converted to the encoder's preferred format.
- On Windows/macOS/Linux plugin availability varies; check `gst-inspect-1.0` to confirm available encoders.

Future work
-----------
- Add a gstreamer-rs mode that exposes a native in-process encoder for lower latency (optional).
- Add unit tests for encoder selection and a streaming integration test on CI using an emulated GPU or software encoder.
