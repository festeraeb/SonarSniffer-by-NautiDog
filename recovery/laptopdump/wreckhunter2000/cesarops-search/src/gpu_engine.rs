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
var input_texture: texture_2d<f32>;

@group(0) @binding(2)
var output_texture: texture_storage_2d<rgba8unorm, write>;

struct Params {
    zion_constant: f32,
    threshold: f32,
    depth_scale: f32,
    padding: f32,
}

@compute @workgroup_size(16, 16)
fn main(@builtin(global_invocation_id) global_id: vec3<u32>) {
    let width = textureDimensions(input_texture).x;
    let height = textureDimensions(input_texture).y;
    
    if global_id.x >= width || global_id.y >= height {
        return;
    }
    
    let center = textureLoad(input_texture, global_id.xy, 0);
    let thermal = center.r;
    
    // Apply Zion Constant depth scaling
    let scaled = thermal * params.zion_constant;
    
    // Apply threshold
    let activated = select(0.0, 1.0, abs(scaled) > params.threshold);
    
    // Output result
    textureStore(output_texture, global_id.xy, vec4<f32>(activated, activated, activated, 1.0));
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
        
        // Create instance
        let instance = Instance::new(InstanceDescriptor {
            backends: Backends::all(),
            ..Default::default()
        });
        
// Enumerate and show all available adapters for debug.
        let mut adapter_candidates: Vec<Adapter> = instance.enumerate_adapters(Backends::all()).into_iter().collect();
        if adapter_candidates.is_empty() {
            return Err("Failed to find any GPU adapters".to_string());
        }
        println!("  Available GPU adapters:");
        for a in &adapter_candidates {
            let info = a.get_info();
            println!("    - {} ({:?}) vendor=0x{:04x} device=0x{:04x}", info.name, info.backend, info.vendor, info.device);
        }

        // Prefer Quadro M2200 by device name and NVIDIA vendor
        let adapter = adapter_candidates.iter().find(|a| {
            let info = a.get_info();
            info.device_type == DeviceType::DiscreteGpu
                && (info.name.to_lowercase().contains("quadro m2200") || info.vendor == 0x10de)
        }).cloned();

        let adapter = match adapter {
            Some(a) => {
                println!("  ✓ Selected Quadro M2200/NVIDIA discrete GPU adapter by preference.");
                a
            }
            None => {
                println!("  ⚠️ Quadro M2200 not found; selecting first discrete GPU adapter.");
                adapter_candidates.into_iter().find(|a| a.get_info().device_type == DeviceType::DiscreteGpu)
                    .or_else(|| {
                        println!("  ⚠️ No discrete GPU; falling back to integrated/compatibility adapter");
                        instance.request_adapter(&RequestAdapterOptions {
                            power_preference: PowerPreference::HighPerformance,
                            force_fallback_adapter: false,
                            compatible_surface: None,
                        })
                    })
                    .ok_or("Failed to find suitable GPU adapter")?
            }
        };

        let adapter_info = adapter.get_info();
        println!("  ✓ GPU Adapter: {} ({:?}), vendor=0x{:04x}, device=0x{:04x}", adapter_info.name, adapter_info.backend, adapter_info.vendor, adapter_info.device);
        if adapter_info.name.to_lowercase().contains("quadro m2200") {
            println!("  🟢 Quadro M2200 is active.");
        } else {
            println!("  🔴 WARNING: Non-Quadro adapter selected; CPU fallback or incorrect card may be in use.");
        }
        
        // Request device
        let (device, queue) = adapter
            .request_device(
                &DeviceDescriptor {
                    required_features: Features::empty(),
                    required_limits: Limits::default(),
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
                BindGroupLayoutEntry {
                    binding: 1,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::Texture {
                        sample_type: TextureSampleType::Float { filterable: true },
                        view_dimension: TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                BindGroupLayoutEntry {
                    binding: 2,
                    visibility: ShaderStages::COMPUTE,
                    ty: BindingType::StorageTexture {
                        access: StorageTextureAccess::WriteOnly,
                        format: TextureFormat::Rgba8Unorm,
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
            cache: None,
        });
        
        println!("  ✓ Compute pipeline created");
        
        // Create params buffer
        let params = Params {
            zion_constant: 1.47,
            threshold: 2.5,
            depth_scale: 1.0,
            padding: 0.0,
        };
        
        let params_buffer = device.create_buffer_init(&BufferInitDescriptor {
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
    
    /// Process thermal TIFF data on GPU
    pub fn process_thermal(&self, thermal_data: &[f32], width: u32, height: u32) -> Result<Vec<f32>, String> {
        println!("  Processing {}x{} thermal data on GPU...", width, height);
        
        // Create input texture
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
            usage: TextureUsages::TEXTURE_BINDING | TextureUsages::COPY_DST,
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
        
        // Create output texture
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
            format: TextureFormat::Rgba8Unorm,
            usage: TextureUsages::STORAGE_BINDING | TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        
        // Create input texture view
        let input_view = input_texture.create_view(&TextureViewDescriptor::default());
        
        // Create output texture view
        let output_view = output_texture.create_view(&TextureViewDescriptor::default());
        
        // Create bind group
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
        
        // Read back results
        let output_buffer = self.device.create_buffer(&BufferDescriptor {
            label: Some("Output Buffer"),
            size: (width * height * 4) as u64,
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
                    bytes_per_row: Some(width * 4),
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
        self.queue.submit(Some(encoder.finish()));
        
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
        let result: Vec<f32> = bytemuck::cast_slice(&data).to_vec();
        
        drop(data);
        output_buffer.unmap();
        
        println!("  ✓ GPU processing complete");
        
        Ok(result)
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
// TIFF LOADING
// ============================================================================

pub fn load_tiff_f32(path: &Path) -> Result<(Vec<f32>, u32, u32), String> {
    use tiff::decoder::Decoder;
    use std::fs::File;
    
    let file = File::open(path).map_err(|e| format!("Failed to open TIFF: {}", e))?;
    let mut decoder = Decoder::new(file).map_err(|e| format!("Failed to decode TIFF: {}", e))?;
    
    let width = decoder.dimensions().map_err(|e| e.to_string())?.0;
    let height = decoder.dimensions().map_err(|e| e.to_string())?.1;
    
    let mut buf = vec![0u16; (width * height) as usize];
    decoder.read_image(&mut tiff::decoder::DecodingResult::U16)
        .map_err(|e| e.to_string())?;
    
    // Convert u16 to f32
    let float_data: Vec<f32> = buf.iter().map(|&v| v as f32).collect();
    
    Ok((float_data, width, height))
}

// ============================================================================
// EXAMPLE USAGE
// ============================================================================

#[tokio::main]
async fn main() {
    println!("================================================================================");
    println!("CESAROPS GPU ENGINE - wgpu Accelerated Processing");
    println!("================================================================================");
    println!();
    
    // Initialize GPU
    let mut engine = GpuEngine::new().await.expect("Failed to initialize GPU");
    
    // Load thermal TIFF
    let thermal_path = Path::new(r"C:\Users\thomf\programming\wreckhunter2000\data\cache\census_raw\2021_low_water\HLS.L30.T16TDN.2021182T162824.v2.0.B10.tif");
    
    if thermal_path.exists() {
        println!("Loading thermal data from: {:?}", thermal_path);
        
        match load_tiff_f32(thermal_path) {
            Ok((data, width, height)) => {
                println!("Loaded {}x{} thermal data", width, height);
                
                // Process on GPU
                match engine.process_thermal(&data, width, height) {
                    Ok(result) => {
                        println!("Processed {} pixels", result.len());
                        
                        // Count anomalies
                        let anomaly_count = result.iter().filter(|&&v| v > 0.5).count();
                        println!("Detected {} anomalies (Z > threshold)", anomaly_count);
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
