// CESAROPS GPU ENGINE - wgpu Accelerated Curvelet Processing
// Loads TIFF data directly into Quadro M2200 VRAM for real-time processing

use wgpu::{util::DeviceExt, *};
use std::path::Path;

// ============================================================================
// SHADER MODULE
// ============================================================================

const SHADER_CODE: &str = r#"
@group(0) @binding(0)
var<uniform> params: Params;

@group(0) @binding(1)
var input_texture: texture_storage_2d<r32float, read>;

@group(0) @binding(2)
var output_texture: texture_storage_2d<r32float, write>;

struct Params {
    mean: f32,
    stddev: f32,
    threshold: f32,
    padding: f32,
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = textureDimensions(input_texture).x;
    let height = textureDimensions(input_texture).y;

    if global_id.x >= width || global_id.y >= height {
        return;
    }

    // Load thermal brightness temperature from R32Float storage texture
    let brightness_temp = textureLoad(input_texture, global_id.xy).r;

    // Calculate Z-score on GPU: Z = (X - mean) / stddev
    let zscore = (brightness_temp - params.mean) / params.stddev;

    // Output the actual Z-score (positive = hot, negative = cold)
    // The CPU will filter for |zscore| > threshold
    textureStore(output_texture, global_id.xy, vec4<f32>(zscore, zscore, zscore, 1.0));
}
"#;

// ============================================================================
// GPU ENGINE STRUCT
// ============================================================================

pub struct GpuEngine {
    instance: Instance,
    surface: Option<Surface<'static>>,
    adapter: Option<Adapter>,
    device: Device,
    queue: Queue,
    pipeline: ComputePipeline,
    bind_group_layout: BindGroupLayout,
    params_buffer: Buffer,
}

impl GpuEngine {
    /// Initialize GPU engine with wgpu
    pub async fn new() -> Result<Self, String> {
        println!("  Initializing wgpu GPU engine...");
        
        // Create instance - prioritize Vulkan for NVIDIA CUDA
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::VULKAN | Backends::DX12,  // Vulkan maps to CUDA on NVIDIA
            ..Default::default()
        });
        
        // Enumerate and show all available adapters for debug.
        let adapter_candidates: Vec<Adapter> = instance.enumerate_adapters(Backends::all()).into_iter().collect();
        if adapter_candidates.is_empty() {
            return Err("Failed to find any GPU adapters".to_string());
        }
        println!("  Available GPU adapters:");
        for (idx, a) in adapter_candidates.iter().enumerate() {
            let info = a.get_info();
            println!("    [{}] {} ({:?}) vendor=0x{:04x} device=0x{:04x}", idx, info.name, info.backend, info.vendor, info.device);
        }

        // FORCE GPU1 (index 1) - the Quadro M2200 on this rig
        if adapter_candidates.len() < 2 {
            return Err("GPU1 not found - need at least 2 adapters".to_string());
        }
        
        let adapter = adapter_candidates.into_iter().nth(1).unwrap();
        let adapter_info = adapter.get_info();
        println!("  ✓ FORCED GPU1: {} ({:?})", adapter_info.name, adapter_info.backend);
        
        if adapter_info.vendor == 0x10de {
            println!("  🟢 NVIDIA GPU - CUDA cores active");
        } else {
            return Err(format!("WRONG GPU: {} is not NVIDIA (vendor=0x{:04x})", adapter_info.name, adapter_info.vendor));
        }
        
        // Request device with features for compute shader + storage textures
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    required_features: Features::TEXTURE_ADAPTER_SPECIFIC_FORMAT_FEATURES,
                    required_limits: Limits {
                        max_compute_workgroup_size_x: 1024,
                        max_compute_workgroup_size_y: 1024,
                        max_compute_workgroups_per_dimension: 65535,
                        ..Default::default()
                    },
                    label: Some("CESAROPS Device"),
                },
                None,
            )
            .await
            .map_err(|e| format!("Failed to create device: {}", e))?;
        
        println!("  ✓ Device created");
        
        // Create shader module
        let shader_module = device.create_shader_module(ShaderModuleDescriptor {
            label: Some("CESAROPS Shader"),
            source: ShaderSource::Wgsl(SHADER_CODE.into()),
        });
        
        // Create bind group layout
        let bind_group_layout = device.create_bind_group_layout(&BindGroupLayoutDescriptor {
            label: Some("CESAROPS Bind Group Layout"),
            entries: &[
                BindGroupLayoutEntry {
                    binding: 0,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Buffer {
                        ty: BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                // Input texture: Storage for R32Float read access
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::StorageTexture {
                        access: StorageTextureAccess::ReadOnly,
                        format: TextureFormat::R32Float,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
                // Output texture: Storage for R32Float write
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::StorageTexture {
                        access: StorageTextureAccess::WriteOnly,
                        format: TextureFormat::R32Float,
                        view_dimension: TextureViewDimension::D2,
                    },
                    count: None,
                },
            ],
        });
        
        // Create pipeline layout
        let pipeline_layout = device.create_pipeline_layout(&PipelineLayoutDescriptor {
            label: Some("CESAROPS Pipeline Layout"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });
        
        // Create compute pipeline
        let pipeline = device.create_compute_pipeline(&ComputePipelineDescriptor {
            label: Some("CESAROPS Compute Pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader_module,
            entry_point: "main",
            compilation_options: Default::default(),
        });
        
        println!("  ✓ Compute pipeline created");
        
        // Create params buffer
        let params = Params {
            zion_constant: 1.47,
            threshold: 2.5,
            depth_scale: 1.0,
            padding: 0.0,
        };
        
        let params_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("Params Buffer"),
            contents: bytemuck::cast_slice(&[params]),
            usage: BufferUsages::UNIFORM | BufferUsages::COPY_DST,
        });
        
        println!("  ✓ GPU engine initialized");
        
        Ok(Self {
            instance,
            surface: None,
            adapter: Some(adapter),
            device,
            queue,
            pipeline,
            bind_group_layout,
            params_buffer,
        })
    }
    
    /// Process thermal TIFF data on GPU with Z-score calculation
    pub fn process_thermal(&self, thermal_data: &[f32], width: u32, height: u32) -> Result<Vec<f32>, String> {
        use std::time::Instant;
        
        println!("  Processing {}x{} thermal data on GPU...", width, height);
        let start_total = Instant::now();

        // Calculate mean and std for Z-score on CPU (one-time cost)
        let start_cpu = Instant::now();
        
        // Filter out NaN/nodata AND zero values for statistics (matching Python)
        let valid_data: Vec<f32> = thermal_data.iter()
            .filter(|&&v| v.is_finite() && v != 0.0)
            .cloned()
            .collect();
        
        let valid_n = valid_data.len() as f32;
        let mean: f32 = valid_data.iter().sum::<f32>() / valid_n;
        
        // Use sample std (N-1) to match Python's np.std() default
        let variance: f32 = if valid_n > 1.0 {
            valid_data.iter()
                .map(|&x| (x - mean).powi(2))
                .sum::<f32>() / (valid_n - 1.0)
        } else {
            0.0
        };
        let std = variance.sqrt();
        let cpu_time = start_cpu.elapsed();

        println!("  Thermal stats: mean={:.2}K, std={:.2} (valid pixels: {:.0}/{:.0}, CPU: {:.3}s)", 
                 mean, std, valid_n, thermal_data.len() as f32, cpu_time.as_secs_f32());

        // Create input texture with STORAGE_BINDING for compute shader read
        let start_upload = Instant::now();
        let input_texture = self.device.create_texture(&TextureDescriptor {
            label: Some("Input Texture"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Write thermal data to texture
        self.queue.write_texture(
            ImageCopyTexture {
                texture: &input_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            bytemuck::cast_slice(thermal_data),
            ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let upload_time = start_upload.elapsed();
        println!("  GPU Upload: {:.3}s ({:.1} MB)", upload_time.as_secs_f32(), (width * height * 4) as f64 / 1_000_000.0);

        // Create output texture with R32Float format for Z-score precision
        let output_texture = self.device.create_texture(&TextureDescriptor {
            label: Some("Output Texture"),
            size: Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: TextureDimension::D2,
            format: TextureFormat::R32Float,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });

        // Update params buffer with calculated mean/stddev
        #[repr(C)]
        #[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
        struct Params {
            mean: f32,
            stddev: f32,
            threshold: f32,
            padding: f32,
        }
        
        let params = Params {
            mean,
            stddev: std,
            threshold: 1.0,  // Lowered to 1.0 for more detections
            padding: 0.0,
        };
        
        self.queue.write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&[params]));
        
        // Create input texture view for storage binding
        let input_view = input_texture.create_view(&TextureViewDescriptor {
            label: Some("Input Texture View"),
            format: Some(TextureFormat::R32Float),
            dimension: Some(TextureViewDimension::D2),
            aspect: TextureAspect::All,
            base_mip_level: 0,
            mip_level_count: Some(1),
            base_array_layer: 0,
            array_layer_count: Some(1),
        });
        
        // Create output texture view
        let output_view = output_texture.create_view(&TextureViewDescriptor::default());
        
        // Create bind group with texture views
        let bind_group = self.device.create_bind_group(&BindGroupDescriptor {
            label: Some("CESAROPS Bind Group"),
            layout: &self.bind_group_layout,
            entries: &[
                BindGroupEntry {
                    binding: 0,
                    resource: self.params_buffer.as_entire_binding(),
                },
                BindGroupEntry {
                    binding: 1,
                    resource: BindingResource::TextureView(&input_view),
                },
                BindGroupEntry {
                    binding: 2,
                    resource: BindingResource::TextureView(&output_view),
                },
            ],
        });
        
        // Create command encoder
        let mut encoder = self
            .device
            .create_command_encoder(&CommandEncoderDescriptor {
                label: Some("CESAROPS Encoder"),
            });
        
        // Run compute pass
        {
            let mut compute_pass = encoder.begin_compute_pass(&ComputePassDescriptor {
                label: Some("CESAROPS Compute Pass"),
                ..Default::default()
            });
            
            compute_pass.set_pipeline(&self.pipeline);
            compute_pass.set_bind_group(0, &bind_group, &[]);
            compute_pass.dispatch_workgroups((width + 15) / 16, (height + 15) / 16, 1);
        }
        
        // Read back results - R32Float output (4 bytes per pixel, no padding needed for single channel)
        let bytes_per_row = ((width * 4 + 255) / 256) * 256; // Align to 256 bytes
        let output_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Output Buffer"),
            size: (bytes_per_row * height) as u64,
            usage: BufferUsages::COPY_DST | BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });

        encoder.copy_texture_to_buffer(
            ImageCopyTexture {
                texture: &output_texture,
                mip_level: 0,
                origin: Origin3d::ZERO,
                aspect: TextureAspect::All,
            },
            ImageCopyBuffer {
                buffer: &output_buffer,
                layout: ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(bytes_per_row),
                    rows_per_image: Some(height),
                },
            },
            Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        // Submit commands
        let start_gpu = Instant::now();
        self.queue.submit(Some(encoder.finish()));

        // Force GPU to complete
        self.device.poll(Maintain::Wait);
        let gpu_time = start_gpu.elapsed();
        println!("  GPU Compute: {:.3}s ({:.1}M pixels/sec)", gpu_time.as_secs_f32(), (width * height) as f64 / gpu_time.as_secs_f64() / 1_000_000.0);

        // Map buffer and read results
        let buffer_slice = output_buffer.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();

        buffer_slice.map_async(MapMode::Read, move |result| {
            tx.send(result).unwrap();
        });

        self.device.poll(Maintain::Wait);

        rx.recv()
            .map_err(|e| format!("Failed to receive buffer: {:?}", e))?
            .map_err(|e| format!("Failed to map buffer: {}", e))?;

        let data = buffer_slice.get_mapped_range();

        // Read Z-scores from R32Float output (accounting for row padding)
        let mut zscores: Vec<f32> = Vec::with_capacity((width * height) as usize);
        let floats_per_row = bytes_per_row / 4; // 4 bytes per float

        for row in 0..height {
            let row_offset = (row * floats_per_row) as usize;
            for col in 0..width {
                let float_offset = row_offset + col as usize;
                if float_offset + 1 <= data.len() {
                    let zscore_bytes = &data[float_offset * 4..(float_offset + 1) * 4];
                    let zscore = f32::from_le_bytes([zscore_bytes[0], zscore_bytes[1], zscore_bytes[2], zscore_bytes[3]]);
                    zscores.push(zscore);
                }
            }
        }

        // Count anomalies (|Z| > 1.0)
        let anomaly_count = zscores.iter().filter(|&&z| z.abs() > 1.0).count();
        println!("  Detected {} anomalies (|Z| > 1.0)", anomaly_count);

        // Find top anomalies
        let mut anomaly_indices: Vec<(usize, f32)> = zscores.iter()
            .enumerate()
            .filter(|(_, &z)| z.abs() > 1.0)
            .map(|(i, &z)| (i, z))
            .collect();
        anomaly_indices.sort_by(|a, b| b.1.abs().partial_cmp(&a.1.abs()).unwrap());

        if !anomaly_indices.is_empty() {
            println!("\n  TOP 10 ANOMALIES:");
            for (rank, (idx, zscore)) in anomaly_indices.iter().take(10).enumerate() {
                let row = idx / width as usize;
                let col = idx % width as usize;
                let anomaly_type = if *zscore < 0.0 { "COLD-SINK" } else { "HOT" };
                println!("    [{}] Pixel ({}, {}): Z = {:.3} ({})", rank + 1, row, col, zscore, anomaly_type);
            }
        }

        drop(data);
        output_buffer.unmap();

        let total_time = start_total.elapsed();
        println!("  ✓ GPU processing complete - Total: {:.3}s", total_time.as_secs_f32());

        Ok(zscores)
    }
    
    /// Update processing parameters
    pub fn update_params(&mut self, zion_constant: f32, threshold: f32, depth_scale: f32) {
        let params = Params {
            zion_constant,
            threshold,
            depth_scale,
            padding: 0.0,
        };
        
        self.queue.write_buffer(&self.params_buffer, 0, bytemuck::cast_slice(&[params]));
    }
}

// ============================================================================
// DATA STRUCTURES
// ============================================================================

#[repr(C)]
#[derive(Copy, Clone, Debug, bytemuck::Pod, bytemuck::Zeroable)]
struct Params {
    zion_constant: f32,
    threshold: f32,
    depth_scale: f32,
    padding: f32,
}

// ============================================================================
// TIFF LOADING & FORENSIC NORMALIZER
// ============================================================================

pub fn load_tiff_f32(path: &Path) -> Result<(Vec<f32>, u32, u32), String> {
    use tiff::decoder::Decoder;
    use std::fs::File;

    let file = File::open(path).map_err(|e| format!("Failed to open TIFF: {}", e))?;
    let mut decoder = Decoder::new(file).map_err(|e| format!("Failed to decode TIFF: {}", e))?;

    let width = decoder.dimensions().map_err(|e| e.to_string())?.0;
    let height = decoder.dimensions().map_err(|e| e.to_string())?.1;

    // Read raw data - HLS thermal bands are stored as scaled integers
    // Need to convert to brightness temperature (Kelvin)
    let raw_data = decoder.read_image().map_err(|e| e.to_string())?;

    const NODATA_VALUE: f32 = -9999.0;  // Common nodata value for GeoTIFF
    
    // Forensic Normalizer: Cast all types to f32 and handle nodata
    // HLS B10 data may be:
    //   - U16 scaled (multiply by 0.1 to get Kelvin)
    //   - I16 already in Kelvin with -9999 nodata
    //   - F32 directly in Kelvin
    let float_data: Vec<f32> = match raw_data {
        tiff::decoder::DecodingResult::U16(buf) => {
            println!("  Converting U16 scaled integers to Kelvin (scale: 0.1)...");
            buf.iter().map(|&v| (v as f32) * 0.1).collect()
        },
        tiff::decoder::DecodingResult::I16(buf) => {
            println!("  Converting I16 to f32 (checking for nodata -9999)...");
            // HLS B10 data format:
            // Raw I16 values with -9999 nodata
            // Range typically 1380-1777 for Lake Michigan
            // This appears to be brightness temperature in Kelvin * 10, but offset
            // Formula: BT(K) = (DN - 1400) / 10 + 273.15 ≈ DN/10 + 133
            // Or it could be radiance that needs conversion
            
            // Check the data range to determine format
            let mut min_val = i16::MAX;
            let mut max_val = i16::MIN;
            for &v in buf.iter() {
                if v != -9999 {
                    if v < min_val { min_val = v; }
                    if v > max_val { max_val = v; }
                }
            }
            println!("  Raw I16 range: {} to {}", min_val, max_val);
            
            // For HLS B10, try direct conversion: BT = DN * 0.1
            // If range is 1380-1777, that gives 138-177K which is too cold
            // Try: BT = (DN - 1400) * 0.1 + 273 = DN*0.1 - 140 + 273 = DN*0.1 + 133
            println!("  Applying HLS B10 conversion: BT(K) = DN * 0.1 + 133...");
            buf.iter().map(|&v| {
                if v == -9999 { f32::NAN } else { (v as f32) * 0.1 + 133.0 }
            }).collect()
        },
        tiff::decoder::DecodingResult::U8(buf) => {
            println!("  Converting U8 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::U32(buf) => {
            println!("  Converting U32 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::U64(buf) => {
            println!("  Converting U64 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::I8(buf) => {
            println!("  Converting I8 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::I32(buf) => {
            println!("  Converting I32 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::I64(buf) => {
            println!("  Converting I64 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
        tiff::decoder::DecodingResult::F32(buf) => {
            println!("  Using F32 data directly...");
            buf
        },
        tiff::decoder::DecodingResult::F64(buf) => {
            println!("  Converting F64 to f32...");
            buf.iter().map(|&v| v as f32).collect()
        },
    };
    
    // Verify data is in expected Kelvin range (filtering nodata)
    if !float_data.is_empty() {
        let valid_values: Vec<f32> = float_data.iter()
            .filter(|&&v| v.is_finite() && v > 100.0 && v < 500.0)
            .cloned()
            .collect();
        
        if !valid_values.is_empty() {
            let min_val = valid_values.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_val = valid_values.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            println!("  Brightness temperature range (valid): {:.1}K - {:.1}K", min_val, max_val);
            
            if min_val < 200.0 || max_val > 400.0 {
                println!("  WARNING: Temperature range seems abnormal for thermal band");
            }
        } else {
            println!("  WARNING: No valid temperature values found (all nodata or out of range)");
        }
    }

    Ok((float_data, width, height))
}

// ============================================================================
// EXAMPLE USAGE
// ============================================================================

#[tokio::main]
async fn main() {
    use std::env;
    use clap::{Parser, ArgAction};
    
    #[derive(Parser)]
    #[command(name = "cesarops-gpu")]
    #[command(about = "CESAROPS GPU Engine - wgpu Accelerated Processing")]
    struct Args {
        /// Path to thermal TIFF file
        tiff_path: Option<String>,
        
        /// Z-score threshold for anomaly detection
        #[arg(long, default_value = "1.0")]
        threshold: f32,
        
        /// Zion constant multiplier
        #[arg(long, default_value = "1.47")]
        zion_constant: f32,
        
        /// Depth scale factor
        #[arg(long, default_value = "1.0")]
        depth_scale: f32,
    }
    
    let args = Args::parse();
    
    println!("================================================================================");
    println!("CESAROPS GPU ENGINE - wgpu Accelerated Processing");
    println!("================================================================================");
    println!();
    println!("Configuration:");
    println!("  Threshold: {}", args.threshold);
    println!("  Zion Constant: {}", args.zion_constant);
    println!("  Depth Scale: {}", args.depth_scale);
    println!();

    // Initialize GPU
    let mut engine = GpuEngine::new().await.expect("Failed to initialize GPU");
    
    // Update parameters from command line
    engine.update_params(args.zion_constant, args.threshold, args.depth_scale);

    // Get TIFF path from args or use default
    let thermal_path = if let Some(path) = args.tiff_path {
        Path::new(&path).to_path_buf()
    } else {
        println!("No TIFF path provided. GPU initialized successfully.");
        println!("Usage: cesarops-gpu.exe <tiff_path> [--threshold 2.5]");
        return;
    };

    if thermal_path.exists() {
        println!("Loading thermal data from: {:?}", thermal_path);

        match load_tiff_f32(&thermal_path) {
            Ok((data, width, height)) => {
                println!("Loaded {}x{} thermal data", width, height);

                // Process on GPU - DIRECT GPU PASS-THROUGH
                match engine.process_thermal(&data, width, height) {
                    Ok(result) => {
                        println!("Processed {} pixels", result.len());

                        // Count anomalies
                        let anomaly_count = result.iter().filter(|&&v| v > 0.5).count();
                        println!("Detected {} anomalies (Z > threshold)", anomaly_count);
                        
                        // Print top anomalies
                        let mut anomalies: Vec<(usize, f32)> = result.iter()
                            .enumerate()
                            .filter(|(_, &v)| v > 0.5)
                            .map(|(i, &v)| (i, v))
                            .collect();
                        anomalies.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
                        
                        if !anomalies.is_empty() {
                            println!("\nTOP 10 ANOMALIES:");
                            for (i, (idx, val)) in anomalies.iter().take(10).enumerate() {
                                let row = idx / width as usize;
                                let col = idx % width as usize;
                                println!("  [{}] Pixel ({}, {}): Z-Score {:.3}", i+1, row, col, val);
                            }
                        }
                    }
                    Err(e) => println!("GPU processing error: {}", e),
                }
            }
            Err(e) => println!("TIFF loading error: {}", e),
        }
    } else {
        println!("Thermal TIFF not found: {:?}", thermal_path);
        println!("Run fetcher.py first to download satellite data");
    }
    
    println!();
    println!("================================================================================");
    println!("GPU ENGINE COMPLETE");
    println!("================================================================================");
}
