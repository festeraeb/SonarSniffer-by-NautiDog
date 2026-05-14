//! wgpu device initialization, pipeline compilation, and GPU matmul dispatch.
//! Uses f32 shader — works on ALL Vulkan GPUs (Pascal, Maxwell, Kelvin, AMD, Intel).
//! f16 shader will be enabled once wgpu/Naga adds support (tracked: gfx-rs/wgpu#4384).

use crate::wgpu_uniform::MatrixDimensions;

pub struct GpuContext {
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub matmul_pipeline: wgpu::ComputePipeline,
    pub bind_group_layout: wgpu::BindGroupLayout,
    pub buf_a: wgpu::Buffer,
    pub buf_b_t: wgpu::Buffer,
    pub buf_c: wgpu::Buffer,
    pub buf_dims: wgpu::Buffer,
    pub max_n: usize,
    pub max_k: usize,
}

impl GpuContext {
    /// Initialize wgpu device on a specific GPU index.
    pub async fn init(gpu_index: usize) -> Result<Self, String> {
        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::VULKAN,
            ..Default::default()
        });

        let adapters: Vec<_> = instance.enumerate_adapters(wgpu::Backends::VULKAN)
            .into_iter()
            .filter(|a| a.get_info().device_type == wgpu::DeviceType::DiscreteGpu)
            .collect();

        if adapters.is_empty() {
            return Err("No discrete Vulkan GPUs found.".to_string());
        }

        let adapter = if gpu_index < adapters.len() {
            &adapters[gpu_index]
        } else {
            tracing::warn!("GPU index {} out of range ({}), using GPU 0", gpu_index, adapters.len());
            &adapters[0]
        };

        let info = adapter.get_info();
        tracing::info!("Selected GPU {}: {} ({:?})", gpu_index, info.name, info.backend);

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("cesarops_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits {
                    max_buffer_size: 1024 * 1024 * 1024, // 1GB
                    max_storage_buffer_binding_size: 1024 * 1024 * 1024,
                    ..Default::default()
                },
                memory_hints: wgpu::MemoryHints::Performance,
            }, None)
            .await
            .map_err(|e| format!("Device request failed: {}", e))?;

        // Load f32 matmul shader (universal, no f16 extension needed)
        let shader_source = include_str!("../shaders/matmul_f32.wgsl");

        let shader_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("matmul_f32"),
            source: wgpu::ShaderSource::Wgsl(shader_source.into()),
        });

        let bind_group_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("matmul_bgl"),
            entries: &[
                bgl_storage(0, true),
                bgl_storage(1, true),
                bgl_storage(2, false),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("matmul_pl"),
            bind_group_layouts: &[&bind_group_layout],
            push_constant_ranges: &[],
        });

        let matmul_pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("matmul_pipeline"),
            layout: Some(&pipeline_layout),
            module: &shader_module,
            entry_point: Some("main"),
            cache: None,
            compilation_options: Default::default(),
        });

        // Pre-allocate for largest matmul: lm_head 1×1536 × 151936×1536
        let max_k: usize = 1536;
        let max_n: usize = 152064; // vocab_size (may be larger than embedding cols)

        // Pre-allocate for largest matmul inputs:
        // - A input: max is FFN intermediate (8960 for 1.5B, could be larger for bigger models)
        // - B_T: max is lm_head weight (vocab_size × hidden_dim)
        // - C output: max is lm_head output (vocab_size)
        let max_input_dim: usize = 8960.max(max_k as usize); // FFN intermediate or hidden
        let max_m: usize = 1;

        let buf_a = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buf_a"),
            size: (max_m * max_input_dim * 4) as u64, // f32, sized for largest input
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // B_T: largest is lm_head (151936 × 1536) but FFN gate/up is (8960 × 1536)
        // lm_head dominates at ~890MB
        let max_bt_elements = (max_n * max_k as usize).max(max_input_dim * max_k as usize);
        let buf_b_t = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buf_b_t"),
            size: (max_bt_elements * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // C output: largest is lm_head output (1 × 151936)
        let max_output = max_n.max(max_input_dim);
        let buf_c = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buf_c"),
            size: (max_m * max_output * 4) as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });

        let buf_dims = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("buf_dims"),
            size: std::mem::size_of::<MatrixDimensions>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        tracing::info!("GPU context ready: {} (~{}MB VRAM for buffers)",
            info.name, (max_n * max_k * 4) / (1024 * 1024));

        Ok(Self { device, queue, matmul_pipeline, bind_group_layout, buf_a, buf_b_t, buf_c, buf_dims, max_n, max_k })
    }

    /// Execute matmul on GPU: C[m,n] = A[m,k] × B_T[n,k]^T
    /// All data is f32. No f16 conversion needed.
    pub fn matmul_gpu(&self, a_f32: &[f32], b_t_f32: &[f32], m: usize, k: usize, n: usize) -> Vec<f32> {
        // Write input data to GPU
        let a_bytes: &[u8] = bytemuck::cast_slice(a_f32);
        let b_bytes: &[u8] = bytemuck::cast_slice(b_t_f32);
        self.queue.write_buffer(&self.buf_a, 0, a_bytes);
        self.queue.write_buffer(&self.buf_b_t, 0, b_bytes);

        // Write dimensions
        let dims = MatrixDimensions { m: m as u32, k: k as u32, n: n as u32, pad: 0 };
        self.queue.write_buffer(&self.buf_dims, 0, bytemuck::bytes_of(&dims));

        // Bind group
        let bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("matmul_bg"),
            layout: &self.bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: self.buf_a.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: self.buf_b_t.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: self.buf_c.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: self.buf_dims.as_entire_binding() },
            ],
        });

        // Encode compute pass
        let mut encoder = self.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("matmul_enc"),
        });

        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
                label: Some("matmul_pass"),
                timestamp_writes: None,
            });
            pass.set_pipeline(&self.matmul_pipeline);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.dispatch_workgroups(
                ((n + 15) / 16) as u32,
                ((m + 15) / 16) as u32,
                1,
            );
        }

        // Staging buffer for readback
        let output_bytes = (m * n * 4) as u64;
        let staging = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("staging"),
            size: output_bytes,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        encoder.copy_buffer_to_buffer(&self.buf_c, 0, &staging, 0, output_bytes);

        // Submit and wait
        let sub_idx = self.queue.submit(std::iter::once(encoder.finish()));
        let slice = staging.slice(0..output_bytes);

        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| {
            let _ = tx.send(result);
        });

        self.device.poll(wgpu::Maintain::WaitForSubmissionIndex(sub_idx));
        rx.recv()
            .expect("GPU map channel closed")
            .expect("GPU buffer map failed");

        let mapped = slice.get_mapped_range();
        let result: Vec<f32> = bytemuck::cast_slice(&mapped).to_vec();
        drop(mapped);
        staging.unmap();

        result
    }
}

fn bgl_storage(binding: u32, read_only: bool) -> wgpu::BindGroupLayoutEntry {
    wgpu::BindGroupLayoutEntry {
        binding,
        visibility: wgpu::ShaderStages::COMPUTE,
        ty: wgpu::BindingType::Buffer {
            ty: wgpu::BufferBindingType::Storage { read_only },
            has_dynamic_offset: false,
            min_binding_size: None,
        },
        count: None,
    }
}
