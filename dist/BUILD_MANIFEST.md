# SonarSniffer release bundles (v0.8.0)

Built on cesarops2 (2026-05-25). Each zip includes `bin/`, sample `test_files/93SV-UHD-GT56.RSD`, and `README.txt`.

| Platform | Zip | Notes |
|----------|-----|--------|
| Linux x86_64 | `sonarsniffer-linux-x64.zip` | GStreamer enabled (MP4 when runtime installed) |
| Windows x86_64 | `sonarsniffer-windows-x64.zip` | Cross-built; MP4 needs GStreamer on Windows |
| macOS universal2 | `sonarsniffer-macos-universal.zip` | Intel + Apple Silicon; MP4 needs native GStreamer build |

## Rebuild

```bash
bash scripts/build_sonarsniffer_all.sh
```

Or per platform: `build_sonarsniffer_linux.sh`, `build_sonarsniffer_windows.sh`, `build_sonarsniffer_macos.sh`.

macOS uses Docker `ghcr.io/rust-cross/cargo-zigbuild` (Apple SDK + zig).
