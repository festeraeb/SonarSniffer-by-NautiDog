SonarSniffer Linux x86_64 bundle

Binaries:
  bin/sonarsniffer-cli  — probe sonar files
  bin/parse_cli         — full pipeline (mosaic, waterfall, KML, MP4 with GStreamer)

Quick test:
  cd bin
  ./sonarsniffer-cli ../test_files
  ./parse_cli ../test_files/93SV-UHD-GT56.RSD --light --output-dir /tmp/ss-out

MP4 export requires GStreamer 1.x dev/runtime on the host.
