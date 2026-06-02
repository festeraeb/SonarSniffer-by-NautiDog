SonarSniffer macOS bundle (cross-built from Linux)

Binaries:
  bin/sonarsniffer-cli  — probe sonar files
  bin/parse_cli         — pipeline without MP4 (no GStreamer in cross-build)

Quick test (Terminal):
  cd bin
  chmod +x sonarsniffer-cli parse_cli
  ./sonarsniffer-cli ../test_files
  ./parse_cli ../test_files/93SV-UHD-GT56.RSD --light --output-dir /tmp/ss-out

MP4 on macOS: install GStreamer, then rebuild natively:
  brew install gstreamer gst-plugins-base gst-plugins-good
  cargo build --release --features video-gstreamer
