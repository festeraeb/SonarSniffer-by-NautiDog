fn main() {
    // Tauri build step — only needed for desktop app builds
    // For library/CLI builds, this is a no-op
    #[cfg(feature = "tauri")]
    tauri_build::build();
}
