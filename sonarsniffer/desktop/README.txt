SonarSniffer desktop (synced from laptop dump).

UI:     desktop/ui/          (index.html, app.js, styles.css)
Tauri:  desktop/src-tauri/   (Tauri 2 — wire to sonarsniffer_lib at ../..)

GStreamer: not bundled in git; install per OS or ship in MSI resources at pack time.
Rebuild: cd desktop/src-tauri && cargo tauri build

Sync: scripts/sync_sonarsniffer_desktop_from_laptop.sh
