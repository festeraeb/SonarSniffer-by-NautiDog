# Curvelet Denoising — Error Tracking Patch Notes
## What was added (`curvelet_diag.rs` + `lib.rs` changes)

### `src-tauri/src/curvelet_diag.rs`  (new)
A static `Mutex<Vec<CurveletDiagEntry>>` log that every call through the
curvelet pipeline appends to.  Fields per entry:

| field | meaning |
|---|---|
| `tag` | call site label (`waterfall_ch0`, `mosaic_ch1`, `preview`, `estimate`) |
| `width` / `height` | input image size |
| `num_scales` | curvelet decomposition depth |
| `threshold_applied` | 0.0 = estimation-only run |
| `suggested_threshold` | MAD universal `σ·√(2 ln N)` estimate |
| `elapsed_ms` | wall time for the full forward+inverse round-trip |
| `error` | empty = success, else the error string |

### `src-tauri/src/lib.rs`  (updated)
- `pub mod curvelet_diag;` declared
- `preview_curvelet` command now has `eprintln!` at start, after parse, and after
  render so the Tauri DevTools console shows every step
- `run_pipeline_internal` logs `[curvelet] estimating threshold …` and the result
- New **`get_curvelet_diagnostics`** Tauri command: drains the log and returns
  `Vec<CurveletDiagEntry>` as JSON — call it from the browser DevTools with:
  ```js
  await window.__TAURI__.core.invoke('get_curvelet_diagnostics')
  ```

## How to instrument `outputs.rs`
Add the following changes to `curvelet_denoise_gray_image`:

```rust
fn curvelet_denoise_gray_image(img: GrayImage, threshold: f32, tag: &str) -> (GrayImage, f32) {
    use std::time::Instant;
    let t0 = Instant::now();
    let (w, h) = (img.width() as usize, img.height() as usize);
    eprintln!("[curvelet] {tag}: {w}x{h} threshold={threshold:.4}");
    if w < 16 || h < 16 {
        eprintln!("[curvelet] {tag}: image too small, skipping");
        crate::curvelet_diag::push(crate::curvelet_diag::CurveletDiagEntry {
            tag: tag.to_string(), width: w, height: h,
            error: "image too small (< 16px)".to_string(), ..Default::default()
        });
        return (img, 0.0);
    }
    // ... existing arr / num_scales setup ...
    let mut coeffs = match curvelet::curvelet_forward(&arr, num_scales) {
        Ok(c) => c,
        Err(e) => {
            let msg = format!("curvelet_forward failed: {e}");
            eprintln!("[curvelet] {tag}: {msg}");
            crate::curvelet_diag::push(crate::curvelet_diag::CurveletDiagEntry {
                tag: tag.to_string(), width: w, height: h, num_scales,
                error: msg, elapsed_ms: t0.elapsed().as_millis() as u64,
                ..Default::default()
            });
            return (img, 0.0);
        }
    };
    // ... MAD estimation ...
    // at the end:
    crate::curvelet_diag::push(crate::curvelet_diag::CurveletDiagEntry {
        tag: tag.to_string(), width: w, height: h, num_scales,
        threshold_applied: threshold as f64,
        suggested_threshold: suggested as f64,
        elapsed_ms: t0.elapsed().as_millis() as u64,
        error: String::new(),
    });
    eprintln!("[curvelet] {tag}: done in {}ms suggested={suggested:.4}",
        t0.elapsed().as_millis());
    (out, suggested)
}
```

Call sites need to pass a tag string, e.g.:
```rust
// in write_waterfall_per_channel:
let (img, used_threshold) = if denoise {
    curvelet_denoise_gray_image(raw, denoise_threshold,
        &format!("waterfall_ch{ch}"))
} else {
    (raw, 0.0_f32)
};
```

## Testing the diagnostics
1. Build with `cargo tauri dev`
2. Run a file with Curvelet Denoising enabled
3. In the Tauri WebView DevTools console:
   ```js
   const diag = await window.__TAURI__.core.invoke('get_curvelet_diagnostics')
   console.table(diag)
   ```
4. Check `stderr` (the terminal you ran `cargo tauri dev` from) for the
   `[curvelet]` lines
5. If `error` field is non-empty the transform failed at that stage — the
   tag + dimensions + error string tell you exactly which image broke it
