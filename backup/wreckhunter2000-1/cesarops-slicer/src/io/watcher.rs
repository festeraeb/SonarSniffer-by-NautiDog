//! Folder watcher for the preprocessing pipeline.
//!
//! Watches the `PREPROCESS_IN` directory on the G-Armor drive.
//! When a new raw GeoTIFF or `.job` file is dropped in, triggers
//! the GDAL OpenCL warp on the P1000 GPU and writes the aligned
//! Master Stack to `READY_TO_SLICE`.

use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Duration;

use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tracing::{error, info, warn};

use crate::io::gdal_warp::{self, WarpConfig};

/// Watches a directory for new GeoTIFF/`.job` files and triggers GPU warp.
///
/// Each new file is dispatched to its own thread.  A `gpu_lock` mutex serialises
/// the actual GDAL warp calls so only one job runs on the P1000 at a time while
/// the event loop stays free to receive subsequent file-create events immediately.
pub struct PreprocessWatcher {
    watch_path: PathBuf,
    output_dir: PathBuf,
    warp_config: WarpConfig,
    /// Serialises GPU warp jobs (max 1 concurrent on the P1000).
    gpu_lock: Arc<Mutex<()>>,
}

impl PreprocessWatcher {
    pub fn new(watch_path: PathBuf, output_dir: PathBuf) -> Self {
        Self {
            watch_path,
            output_dir,
            warp_config: WarpConfig::default(),
            gpu_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Create with custom warp config.
    pub fn with_config(watch_path: PathBuf, output_dir: PathBuf, warp_config: WarpConfig) -> Self {
        Self {
            watch_path,
            output_dir,
            warp_config,
            gpu_lock: Arc::new(Mutex::new(())),
        }
    }

    /// Start watching. Blocks the calling thread.
    pub fn run(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Watching {:?} for new mission jobs...", self.watch_path);

        let (tx, rx) = mpsc::channel();
        let mut watcher = RecommendedWatcher::new(
            tx,
            Config::default().with_poll_interval(Duration::from_secs(2)),
        )?;

        watcher.watch(&self.watch_path, RecursiveMode::Recursive)?;

        for result in rx {
            match result {
                Ok(event) => {
                    if let EventKind::Create(_) = event.kind {
                        for path in event.paths.iter() {
                            self.on_new_file(path);
                        }
                    }
                }
                Err(e) => warn!("Watch error: {}", e),
            }
        }

        Ok(())
    }

    fn on_new_file(&self, path: &Path) {
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");

        let should_process = matches!(ext, "tif" | "tiff" | "job");
        if !should_process {
            return;
        }

        // Skip temp/lock files
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name.starts_with('~') {
            return;
        }

        info!("New mission job detected: {:?}", path);

        // Generate output path
        let output_name = format!("aligned_{}", path.file_stem().unwrap().to_string_lossy());
        let path_owned = path.to_path_buf();
        let output_path = self.output_dir.join(format!("{}.tif", output_name));
        let warp_config = self.warp_config.clone();
        let gpu_lock = Arc::clone(&self.gpu_lock);
        let err_dir = self
            .output_dir
            .parent()
            .unwrap_or(&self.output_dir)
            .join("WARP_ERRORS");

        // Dispatch to a background thread so the event loop is never blocked
        // by a long-running warp.  The gpu_lock ensures only one warp runs on
        // the P1000 at a time (subsequent dispatches queue up inside the thread).
        std::thread::spawn(move || {
            // Wait for any in-progress GPU job to finish before starting ours
            let _guard = gpu_lock.lock().unwrap();

            match gdal_warp::gpu_warp_with_config(
                &path_owned,
                &output_path,
                None,
                warp_config.clone(),
            ) {
                Ok(warp_info) => {
                    info!(
                        "GPU Warp complete: {:?} → {:?} ({:.1}s, wm={}MB, block={})",
                        path_owned,
                        output_path,
                        warp_info.duration_s,
                        warp_info.working_memory_mb,
                        warp_info.block_size
                    );
                }
                Err(e) => {
                    error!("GPU Warp failed for {:?}: {}", path_owned, e);
                    // OOM: retry with conservative config
                    if let gdal_warp::WarpError::WarpFailed { ref stderr, .. } = e {
                        if stderr.to_lowercase().contains("opencl")
                            || stderr.to_lowercase().contains("memory")
                        {
                            warn!("OpenCL OOM — retrying with conservative config (wm=2000)");
                            let cons_config = WarpConfig::conservative();
                            match gdal_warp::gpu_warp_with_config(
                                &path_owned,
                                &output_path,
                                None,
                                cons_config,
                            ) {
                                Ok(retry_info) => {
                                    info!(
                                        "GPU Warp (conservative) succeeded: {:?} → {:?} ({:.1}s)",
                                        path_owned, output_path, retry_info.duration_s
                                    );
                                    return;
                                }
                                Err(retry_err) => {
                                    error!("Conservative retry also failed: {}", retry_err);
                                }
                            }
                        }
                    }
                    // Move to error folder for later retry
                    std::fs::create_dir_all(&err_dir).ok();
                    if let Some(dest) = err_dir.join(path_owned.file_name().unwrap()).to_str() {
                        std::fs::copy(&path_owned, dest).ok();
                    }
                }
            }
        });
    }
}
