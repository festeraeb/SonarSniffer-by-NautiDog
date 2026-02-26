# Integration Checklist

## 📋 Step-by-Step Integration Guide

### Phase 1: Setup (15 minutes)

- [ ] **Copy module to project**
  ```bash
  cp -r video_enhanced/ /path/to/your/project/src/
  ```

- [ ] **Add dependencies to Cargo.toml**
  ```toml
  [dependencies]
  serde = { version = "1.0", features = ["derive"] }
  anyhow = "1.0"
  
  # Video encoding
  gstreamer = { version = "0.22", optional = true }
  gstreamer-app = { version = "0.22", optional = true }
  gif = "0.12"  # Fallback for non-GStreamer builds
  
  [features]
  video-gstreamer = ["gstreamer", "gstreamer-app"]
  ```

- [ ] **Add module declaration to lib.rs**
  ```rust
  mod video_enhanced;
  pub use video_enhanced::{
      render_enhanced_waterfall,
      render_enhanced_waterfall_auto,
      SonarProcessingParams,
      Colormap,
      EnhancedVideoResult,
  };
  ```

- [ ] **Verify compilation**
  ```bash
  cargo check --features video-gstreamer
  ```

---

### Phase 2: Basic Integration (30 minutes)

- [ ] **Replace old video export in lib.rs**
  
  Find this code:
  ```rust
  let video = video::run_video_export_pings(pings, &output_dir, on_progress);
  ```
  
  Replace with:
  ```rust
  let video = match video_enhanced::render_enhanced_waterfall_auto(
      pings,
      &output_dir,
      on_progress,
  ) {
      Ok(v) => Some(v),
      Err(e) => {
          eprintln!("Enhanced video export failed: {e}");
          None
      }
  };
  ```

- [ ] **Update PipelineResponse struct**
  ```rust
  pub struct PipelineResponse {
      // ... existing fields ...
      pub enhanced_video: Option<EnhancedVideoResult>,
  }
  ```

- [ ] **Test with sample data**
  ```bash
  cargo run -- /path/to/test.rsd
  ```

- [ ] **Verify output files**
  - Check for `sonar_waterfall_enhanced.mp4` (or `.gif`)
  - Compare with original output
  - Verify file size is reasonable

---

### Phase 3: Parameter Exposure (1 hour)

- [ ] **Add Tauri command for enhanced processing**
  ```rust
  #[tauri::command]
  fn run_enhanced_pipeline(
      file_name: &str,
      processing_params: Option<SonarProcessingParams>,
      app: tauri::AppHandle
  ) -> Result<PipelineResponse, String> {
      let params = processing_params.unwrap_or_default();
      
      // ... parse file ...
      
      let video = video_enhanced::render_enhanced_waterfall(
          pings,
          &output_dir,
          params,
          |frame, total| {
              let _ = app.emit("video-progress", json!({
                  "frame": frame,
                  "total": total,
                  "percent": (frame * 100) / total
              }));
          }
      ).map_err(|e| e.to_string())?;
      
      // ... build response ...
  }
  ```

- [ ] **Register command in Tauri builder**
  ```rust
  .invoke_handler(tauri::generate_handler![
      // ... existing commands ...
      run_enhanced_pipeline,
  ])
  ```

- [ ] **Add TypeScript types (frontend)**
  ```typescript
  interface SonarProcessingParams {
    tvgEnabled: boolean;
    tvgSpreadingFactor: number;
    tvgAbsorptionDbPerM: number;
    logCompression: boolean;
    noiseFloorDb: number;
    signalCeilingDb: number;
    useAdaptiveRange: boolean;
    medianFilterEnabled: boolean;
    medianKernelSize: 3 | 5 | 7;
    bilateralFilterEnabled: boolean;
    histogramEqualization: boolean;
    claheEnabled: boolean;
    colormap: 'grayscale' | 'viridis' | 'magma' | 'jet' | 'sonarCustom';
    interpolateGaps: boolean;
    fps: number;
    videoHeight: number;
  }
  ```

- [ ] **Create UI controls (Svelte/React example)**
  ```svelte
  <script>
    let params = {
      tvgEnabled: true,
      tvgSpreadingFactor: 20,
      colormap: 'viridis',
      medianFilterEnabled: true,
      // ... other params with defaults
    };
    
    async function processWithEnhancement() {
      const result = await invoke('run_enhanced_pipeline', {
        fileName: selectedFile,
        processingParams: params
      });
      // Handle result
    }
  </script>
  
  <div class="controls">
    <label>
      <input type="checkbox" bind:checked={params.tvgEnabled} />
      Enable TVG Correction
    </label>
    
    <label>
      Spreading Factor:
      <input type="range" min="10" max="40" step="1"
             bind:value={params.tvgSpreadingFactor} />
      {params.tvgSpreadingFactor}
    </label>
    
    <label>
      Colormap:
      <select bind:value={params.colormap}>
        <option value="grayscale">Grayscale</option>
        <option value="viridis">Viridis</option>
        <option value="magma">Magma</option>
        <option value="jet">Jet</option>
        <option value="sonarCustom">Sonar Custom</option>
      </select>
    </label>
    
    <!-- More controls... -->
  </div>
  ```

---

### Phase 4: Testing & Validation (2 hours)

- [ ] **Unit tests**
  ```bash
  cargo test --lib video_enhanced
  ```
  Expected: All tests pass

- [ ] **Test with real data**
  - [ ] Small file (10k pings)
  - [ ] Medium file (100k pings)
  - [ ] Large file (1M+ pings)

- [ ] **Visual inspection**
  - [ ] Compare original vs enhanced side-by-side
  - [ ] Verify TVG correction works (far-range visible)
  - [ ] Check colormaps render correctly
  - [ ] Confirm no artifacts or distortion

- [ ] **Performance validation**
  - [ ] Measure processing time for typical file
  - [ ] Check memory usage (should be <3× raw file size)
  - [ ] Verify video encoding completes without timeout

- [ ] **Parameter tuning**
  - [ ] Test different TVG spreading factors (15, 20, 25, 30)
  - [ ] Try all colormaps
  - [ ] Compare with/without filtering
  - [ ] Validate adaptive vs fixed dynamic range

---

### Phase 5: Documentation & User Training (1 hour)

- [ ] **Update user documentation**
  - [ ] Add section on enhanced video export
  - [ ] Explain parameters and when to adjust them
  - [ ] Include before/after examples
  - [ ] Add troubleshooting section

- [ ] **Create tooltips in UI**
  ```svelte
  <Tooltip text="Compensates for acoustic intensity loss with range. 
                 Increase for deep water, decrease for shallow water.">
    <label>TVG Spreading Factor</label>
  </Tooltip>
  ```

- [ ] **Add preset buttons**
  ```typescript
  function applyPreset(preset: 'fast' | 'highQuality' | 'shallowWater' | 'deepWater') {
    switch (preset) {
      case 'fast':
        params = { /* minimal settings */ };
        break;
      case 'highQuality':
        params = { /* all enhancements */ };
        break;
      // ... etc
    }
  }
  ```

---

### Phase 6: Production Deployment (30 minutes)

- [ ] **Update build scripts**
  ```toml
  # Cargo.toml
  [profile.release]
  opt-level = 3
  lto = true
  ```

- [ ] **Test release build**
  ```bash
  cargo build --release --features video-gstreamer
  ```

- [ ] **Package for distribution**
  - [ ] Include GStreamer runtime (if using)
  - [ ] Add documentation PDFs
  - [ ] Create installer with dependencies

- [ ] **Create release notes**
  - [ ] Highlight new features
  - [ ] Explain benefits (2-3× better range, etc.)
  - [ ] Link to documentation

---

## 🧪 Test Cases

### Test 1: Shallow Water (5-20 ft)
```bash
Input: shallow_lake_scan.rsd
Expected: Bottom clearly visible, good contrast
Params: tvg_spreading_factor = 15, noise_floor_db = -50
```

### Test 2: Deep Water (100+ ft)
```bash
Input: deep_ocean_scan.rsd
Expected: Far-range returns visible, minimal fade
Params: tvg_spreading_factor = 30, noise_floor_db = -70
```

### Test 3: Structure Scan (Medium Range)
```bash
Input: dock_structure_scan.rsd
Expected: Sharp edges, texture detail visible
Params: clahe_enabled = true, median_kernel_size = 5
```

### Test 4: Noisy Data
```bash
Input: interference_test.rsd
Expected: Clean appearance, speckle removed
Params: bilateral_filter_enabled = true
```

### Test 5: Data with Gaps
```bash
Input: dropout_test.rsd
Expected: Black regions filled or interpolated
Params: interpolate_gaps = true
```

---

## ✅ Success Criteria

- [ ] Enhanced video generated successfully
- [ ] File size reasonable (10-50 MB for typical 1-hour log)
- [ ] Processing completes in <2 minutes for 100k pings
- [ ] Visual quality clearly superior to original
- [ ] Far-range detail visible (2-3× range extension)
- [ ] UI controls responsive and intuitive
- [ ] No crashes or memory issues
- [ ] All unit tests pass

---

## 🐛 Known Issues & Workarounds

### Issue 1: Compilation Error - Missing Ping Fields
**Symptom**: `field 'xyz' not found in Ping struct`

**Fix**: Add missing fields to Ping struct initialization:
```rust
Ping {
    channel,
    depth_ft,
    temp_c,
    samples,
    // Add these if missing:
    timestamp: 0,
    gps_lat: None,
    gps_lon: None,
    frequency_khz: 0,
    // ... other fields as needed
}
```

### Issue 2: GStreamer Not Found
**Symptom**: "Pipeline parse failed"

**Workaround**: Build without GStreamer feature (uses GIF fallback):
```bash
cargo build --release  # Omit --features video-gstreamer
```

Or install GStreamer:
- **Windows**: https://gstreamer.freedesktop.org/download/
- **Linux**: `sudo apt install gstreamer1.0-*`
- **macOS**: `brew install gstreamer gst-plugins-{base,good,bad,ugly}`

### Issue 3: Processing Too Slow
**Symptom**: Takes >5 minutes for 100k pings

**Fix**: Use fast preset or disable heavy processing:
```rust
let params = SonarProcessingParams::fast();
// Or manually:
params.bilateral_filter_enabled = false;
params.clahe_enabled = false;
```

### Issue 4: Over-brightened Output
**Symptom**: Everything looks washed out

**Fix**: Decrease TVG spreading factor or ceiling:
```rust
params.tvg_spreading_factor = 15.0; // Instead of 20
params.signal_ceiling_db = -10.0;   // Instead of 0
```

### Issue 5: Lost Detail in Dark Regions
**Symptom**: Weak returns disappear

**Fix**: Lower noise floor or use adaptive range:
```rust
params.noise_floor_db = -70.0;     // Instead of -60
params.use_adaptive_range = true;  // Automatically adjust
```

---

## 📞 Support Checklist

Before asking for help, verify:

- [ ] All dependencies installed correctly
- [ ] Compilation succeeds without errors
- [ ] Test files are valid Garmin RSD format
- [ ] GStreamer runtime installed (if using video feature)
- [ ] Sufficient disk space (3× input file size)
- [ ] No antivirus blocking file writes
- [ ] Checked console for error messages
- [ ] Reviewed troubleshooting section in docs

---

## 🎯 Next Steps After Integration

### Week 1: Basic Usage
- Get comfortable with default parameters
- Process variety of files (shallow, deep, structures)
- Identify optimal settings for your use cases

### Week 2: Advanced Features
- Experiment with all colormaps
- Try different filtering combinations
- Test CLAHE for challenging scenes

### Week 3: Parameter Tuning
- Create custom presets for common scenarios
- Document best practices for your data
- Train users on parameter adjustment

### Week 4: Optimization
- Profile performance on large files
- Enable parallel processing if needed
- Consider GPU acceleration for heavy filtering

---

## 📊 Success Metrics

Track these to measure improvement:

1. **Processing Time**: Should be <1 min per 100k pings
2. **Output Quality**: User rating 1-10 (target: 8+)
3. **Detection Range**: Measure max usable depth (target: +50%)
4. **User Adoption**: % of users using enhanced mode (target: 80%+)
5. **Support Tickets**: Issues related to video quality (target: -75%)

---

**Estimated Total Integration Time**: 4-6 hours for complete integration and testing

**Priority Tasks** (if time-limited):
1. ✅ Phase 1 & 2: Basic integration (45 mins)
2. ✅ Phase 4: Test with real data (1 hour)
3. ⚠️ Phase 3: UI controls (defer if needed)
4. ⚠️ Phase 5: Documentation (do incrementally)

**Critical Path**: Get it working first, then iterate on parameters and UI!
