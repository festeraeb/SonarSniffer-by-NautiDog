SonarSniffer Windows x64 bundle (cross-built from Linux)

Binaries:
  bin\sonarsniffer-cli.exe  — probe sonar files
  bin\parse_cli.exe         — full pipeline (no MP4 without GStreamer on Windows)

Quick test (PowerShell):
  cd bin
  .\sonarsniffer-cli.exe ..\test_files
  .\parse_cli.exe ..\test_files\93SV-UHD-GT56.RSD --light --output-dir ..\out

MP4 export on Windows:
  Install GStreamer MSVC runtime (full):
  https://gstreamer.freedesktop.org/download/
  Then rebuild on Windows with: cargo build --release --features video-gstreamer
